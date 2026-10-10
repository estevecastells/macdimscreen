//! `dimd` — the MacDimScreen daemon.
//!
//! Runs as the logged-in user under launchd (a LaunchAgent, no root needed).
//! Every `TICK` it works out the colour temperature for the time of day and
//! makes Night Shift match. On exit (SIGTERM, SIGINT, panic) it puts Night
//! Shift back the way it found it.

use dim_core::protocol::{default_socket_path, support_dir};
use dim_core::Config;
use dimd::colorfilter::MediaAccessibility;
use dimd::nightshift::CoreBrightness;
use dimd::{flux, log, server, unix_now, Daemon};
use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
use signal_hook::iterator::Signals;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Small steps every 15 s make transitions imperceptible: at most ~35 K per
/// step, and Night Shift fades each change over 2 s by itself.
const TICK: Duration = Duration::from_secs(15);

const USAGE: &str = "\
usage: dimd [--config PATH] [--socket PATH] [--dry-run] [--once]

  --config PATH   config file (default: ~/Library/Application Support/MacDimScreen/config.toml)
  --socket PATH   control socket (default: ~/Library/Application Support/MacDimScreen/dimd.sock)
  --dry-run       compute the schedule but never touch Night Shift
  --once          run a single tick, print status as JSON, and exit
  --print-default-config
";

struct Args {
    config: PathBuf,
    socket: PathBuf,
    dry_run: bool,
    once: bool,
}

fn parse_args() -> Args {
    let mut args =
        Args { config: support_dir().join("config.toml"), socket: default_socket_path(), dry_run: false, once: false };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--config" => args.config = it.next().unwrap_or_else(|| die("--config needs a path")).into(),
            "--socket" => args.socket = it.next().unwrap_or_else(|| die("--socket needs a path")).into(),
            "--dry-run" => args.dry_run = true,
            "--once" => args.once = true,
            "--print-default-config" => {
                print!("{}", first_run_config().to_toml());
                std::process::exit(0);
            }
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            other => die(&format!("unknown argument {other}\n{USAGE}")),
        }
    }
    args
}

fn die(msg: &str) -> ! {
    eprintln!("dimd: {msg}");
    std::process::exit(2);
}

/// Defaults for a new install: f.lux's settings if present, else a location
/// estimated from the time zone.
fn first_run_config() -> Config {
    let mut cfg = Config::default();
    (cfg.latitude, cfg.longitude) = dimd::estimated_location(unix_now());
    let imported = flux::import(&mut cfg);
    if !imported.is_empty() {
        log!("imported from f.lux: {}", imported.join(", "));
    }
    cfg
}

fn load_config(path: &PathBuf) -> Config {
    match std::fs::read_to_string(path) {
        Ok(s) => Config::from_toml(&s).unwrap_or_else(|e| {
            log!("invalid config {}: {e}; using defaults", path.display());
            first_run_config()
        }),
        Err(_) => {
            log!("no config at {}; creating one", path.display());
            let cfg = first_run_config();
            if let Err(e) = server::save_config(path, &cfg) {
                log!("warning: could not write {}: {e}", path.display());
            }
            cfg
        }
    }
}

/// Restores Night Shift when dropped, including during a panic unwind.
struct RestoreGuard(Arc<server::Shared<CoreBrightness>>);

impl Drop for RestoreGuard {
    fn drop(&mut self) {
        match self.0.daemon().restore() {
            Ok(()) => log!("Night Shift restored to its previous settings"),
            Err(e) => log!("error restoring Night Shift: {e}"),
        }
    }
}

fn main() {
    let args = parse_args();
    let cfg = load_config(&args.config);
    let night_shift =
        if args.dry_run { Err("dry run: Night Shift untouched".to_string()) } else { CoreBrightness::open() };
    log!(
        "dimd {} starting: location {:.2},{:.2}{}, night {} K, Night Shift: {}",
        env!("CARGO_PKG_VERSION"),
        cfg.latitude,
        cfg.longitude,
        if cfg.location_estimated { " (estimated)" } else { "" },
        cfg.night_kelvin,
        night_shift.as_ref().map(|_| "ok").unwrap_or_else(|e| e.as_str()),
    );
    let original = (!args.dry_run && !args.once).then(|| support_dir().join("night-shift-original.json"));
    let filter: Option<dimd::BoxedFilter> = if args.dry_run {
        None
    } else {
        match MediaAccessibility::open() {
            Ok(f) => Some(Box::new(f)),
            Err(e) => {
                log!("extra warmth unavailable: {e}");
                None
            }
        }
    };
    let mut daemon = Daemon::new(cfg, night_shift, filter, original, unix_now());

    if args.once {
        println!("{}", serde_json::to_string_pretty(daemon.tick(unix_now())).unwrap());
        return;
    }

    let shared = server::Shared::new(daemon, Some(args.config.clone()));
    let _guard = RestoreGuard(shared.clone());

    // Signals are handled on their own thread, which wakes the control loop, so the
    // loop can sleep a whole tick instead of waking up to check a flag.
    let term = Arc::new(AtomicBool::new(false));
    let mut signals = Signals::new([SIGTERM, SIGINT, SIGHUP]).expect("register signal handlers");
    {
        let (term, shared) = (term.clone(), shared.clone());
        std::thread::Builder::new()
            .name("signals".into())
            .spawn(move || {
                for _ in signals.forever() {
                    term.store(true, Ordering::Relaxed);
                    shared.kick();
                }
            })
            .expect("spawn signal thread");
    }
    if let Err(e) = server::spawn(shared.clone(), &args.socket) {
        die(&format!("cannot listen on {}: {e}", args.socket.display()));
    }
    log!("listening on {}", args.socket.display());

    let mut last_phase = None;
    while !term.load(Ordering::Relaxed) {
        {
            let mut daemon = shared.daemon();
            let mode_before = daemon.config().mode.clone();
            let status = daemon.tick(unix_now());
            let phase = status.target.phase;
            if last_phase != Some(phase) {
                log!(
                    "{phase:?}: target {:.0} K, dim {:.0}%, tint {:.0}%, Night Shift at {}",
                    status.target.kelvin,
                    status.target.dim_pct,
                    status.target.tint_pct,
                    status.applied_kelvin.map(|k| format!("{k:.0} K")).unwrap_or_else(|| "off".into()),
                );
                last_phase = Some(phase);
            }
            // A pause that expired switched the mode back to Auto: persist it.
            if daemon.config().mode != mode_before {
                if let Some(path) = shared.config_path() {
                    let _ = server::save_config(path, daemon.config());
                }
            }
        }
        // Wakes early on settings changes and signals.
        shared.wait(TICK);
    }
    log!("shutting down");
    let _ = std::fs::remove_file(&args.socket);
}
