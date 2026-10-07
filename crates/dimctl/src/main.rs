//! `dimctl` — command-line client for the MacDimScreen daemon.

use clap::{Parser, Subcommand};
use dim_core::config::parse_hhmm;
use dim_core::protocol::{default_socket_path, Request, Response, Status};
use dim_core::schedule::{day_plan, local_midnight, Phase};
use dim_core::{Config, Mode};
use dimd::{client, unix_now, utc_offset};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "dimctl", version, about = "Control MacDimScreen: warmer, dimmer screen at night")]
struct Cli {
    /// Daemon socket (default: ~/Library/Application Support/MacDimScreen/dimd.sock)
    #[arg(long, global = true)]
    socket: Option<PathBuf>,
    /// Print raw JSON
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Current colour temperature, phase and next change (default)
    Status,
    /// Today's schedule: sunrise, sunset, transitions and the hourly curve
    Plan,
    /// Follow the schedule (undoes off, pause and manual)
    Auto,
    /// Turn off until `dimctl auto`
    Off,
    /// Turn off for a while, then return to the schedule
    Pause {
        /// Minutes
        #[arg(default_value_t = 60)]
        minutes: i64,
    },
    /// Hold a fixed colour temperature (2700–6500 K) regardless of time
    Manual {
        kelvin: f64,
        /// Extra dimming in percent
        #[arg(long, default_value_t = 0.0)]
        dim: f64,
        /// Extra-warmth colour tint in percent (25–100; 0 = off)
        #[arg(long, default_value_t = 0.0)]
        tint: f64,
    },
    /// Change settings
    Set {
        /// Night colour temperature in kelvin (Night Shift goes down to 2700)
        #[arg(long)]
        night: Option<f64>,
        /// Daytime colour temperature in kelvin (6500 = unchanged)
        #[arg(long)]
        day: Option<f64>,
        /// Extra dimming at night, percent (0–90)
        #[arg(long)]
        dim: Option<f64>,
        /// Extra warmth at night: Accessibility colour tint, percent (25–100; 0 = off)
        #[arg(long)]
        tint: Option<f64>,
        /// Transition length in minutes
        #[arg(long)]
        transition: Option<f64>,
        /// Wake time, "HH:MM", or "sunrise" to follow the sun
        #[arg(long)]
        wake: Option<String>,
        /// "LAT,LON", e.g. 41.39,2.17
        #[arg(long, allow_hyphen_values = true)]
        location: Option<String>,
    },
    /// Print the daemon's configuration as TOML
    Config,
}

fn main() -> ExitCode {
    // Exit quietly when piped into `head` instead of panicking on a closed stdout.
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) };
    let cli = Cli::parse();
    let socket = cli.socket.clone().unwrap_or_else(default_socket_path);
    let send = |req: Request| -> Result<Response, String> {
        match client::request(&socket, &req)? {
            Response::Error { message } => Err(message),
            r => Ok(r),
        }
    };
    let result = match cli.cmd.unwrap_or(Cmd::Status) {
        Cmd::Status => status(&send, cli.json),
        Cmd::Plan => get_config(&send).map(|c| plan(&c)),
        Cmd::Auto => set_mode(&send, Mode::Auto, cli.json),
        Cmd::Off => set_mode(&send, Mode::Off, cli.json),
        Cmd::Pause { minutes } => set_mode(&send, Mode::Paused { until: unix_now() + minutes.max(1) * 60 }, cli.json),
        Cmd::Manual { kelvin, dim, tint } => {
            set_mode(&send, Mode::Manual { kelvin, dim_pct: dim, tint_pct: tint }, cli.json)
        }
        Cmd::Set { night, day, dim, tint, transition, wake, location } => (|| {
            let mut c = get_config(&send)?;
            if let Some(v) = night {
                c.night_kelvin = v;
            }
            if let Some(v) = day {
                c.day_kelvin = v;
            }
            if let Some(v) = dim {
                c.night_dim_pct = v;
            }
            if let Some(v) = tint {
                c.night_tint_pct = v;
            }
            if let Some(v) = transition {
                c.transition_minutes = v;
            }
            if let Some(w) = wake {
                c.wake_time = match w.as_str() {
                    "sunrise" | "none" | "" => None,
                    _ => Some(w),
                };
            }
            if let Some(loc) = location {
                let (lat, lon) = loc.split_once(',').ok_or("location must look like 41.39,2.17")?;
                c.latitude = lat.trim().parse().map_err(|_| "bad latitude")?;
                c.longitude = lon.trim().parse().map_err(|_| "bad longitude")?;
                c.location_estimated = false;
            }
            send(Request::SetConfig { config: Box::new(c) })?;
            status(&send, cli.json)
        })(),
        Cmd::Config => get_config(&send).map(|c| print!("{}", c.to_toml())),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("dimctl: {e}");
            if e.contains("cannot connect") {
                eprintln!("Is the daemon running? Open MacDimScreen.app, or run `make install`.");
            }
            ExitCode::FAILURE
        }
    }
}

type Send<'a> = &'a dyn Fn(Request) -> Result<Response, String>;

fn get_config(send: Send) -> Result<Config, String> {
    match send(Request::GetConfig)? {
        Response::Config { config } => Ok(config),
        other => Err(format!("unexpected response {other:?}")),
    }
}

fn set_mode(send: Send, mode: Mode, json: bool) -> Result<(), String> {
    send(Request::SetMode { mode })?;
    status(send, json)
}

fn status(send: Send, json: bool) -> Result<(), String> {
    let Response::Status { status } = send(Request::Status)? else { return Err("unexpected response".into()) };
    if json {
        println!("{}", serde_json::to_string_pretty(&status).unwrap());
    } else {
        print_status(&status);
    }
    Ok(())
}

fn hhmm(unix: i64) -> String {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&unix, &mut tm) };
    format!("{:02}:{:02}", tm.tm_hour, tm.tm_min)
}

fn phase_name(p: Phase) -> &'static str {
    match p {
        Phase::Day => "Daylight",
        Phase::Sunset => "Sunset transition",
        Phase::Night => "Night",
        Phase::Sunrise => "Morning transition",
        Phase::Paused => "Paused",
        Phase::Off => "Off",
        Phase::Manual => "Manual",
    }
}

fn print_status(s: &Status) {
    let t = &s.target;
    let shown = s.applied_kelvin.map(|k| format!("{k:.0} K")).unwrap_or_else(|| "off (6500 K)".into());
    println!("{:<12}{}", "phase", phase_name(t.phase));
    println!("{:<12}{:.0} K{}", "target", t.kelvin, if s.clamped { " (Night Shift's limit is 2700 K)" } else { "" });
    println!("{:<12}{shown}", "night shift");
    if t.dim_pct > 0.0 {
        println!("{:<12}{:.0}% (menu bar app overlay)", "dimming", t.dim_pct);
    }
    if let Some(tint) = s.applied_tint_pct {
        println!("{:<12}{tint:.0}% colour tint", "warmth+");
    }
    if let (Some(at), next) = (t.next_change, t.next_phase) {
        let what = next.map(phase_name).unwrap_or("schedule resumes");
        println!("{:<12}{what} at {}", "next", hhmm(at));
    }
    let today = &t.today;
    let fmt = |x: Option<i64>| x.map(hhmm).unwrap_or_else(|| "—".into());
    println!("{:<12}sunrise {}, sunset {}", "today", fmt(today.sunrise), fmt(today.sunset));
    println!(
        "{:<12}{:.2}, {:.2}{}",
        "location",
        s.latitude,
        s.longitude,
        if s.location_estimated { " (estimated from time zone; set with `dimctl set --location`)" } else { "" }
    );
    if let Some(e) = &s.last_error {
        println!("{:<12}{e}", "error");
    }
}

fn plan(c: &Config) {
    let now = unix_now();
    let offset = utc_offset(now);
    let midnight = local_midnight(now, offset);
    let p = day_plan(c, midnight);
    let fmt = |x: Option<i64>| x.map(hhmm).unwrap_or_else(|| "—".into());
    let tr = (c.transition_minutes * 60.0) as i64;
    println!("sunrise {}  sunset {}", fmt(p.sunrise), fmt(p.sunset));
    if let (Some(m), Some(e)) = (p.morning, p.evening) {
        let wake = c.wake_time.as_deref().filter(|w| parse_hhmm(w).is_some());
        println!(
            "morning transition {}–{}{}",
            hhmm(m - tr),
            hhmm(m),
            wake.map(|w| format!(" (wake time {w})")).unwrap_or_default()
        );
        println!("evening transition {}–{}", hhmm(e), hhmm(e + tr));
    }
    println!();
    let auto = Config { mode: Mode::Auto, ..c.clone() };
    for h in 0..24 {
        let t = dim_core::target(&auto, midnight + h * 3600, offset);
        let bar = "█".repeat(((6500.0 - t.kelvin.min(6500.0)) / 100.0).round() as usize);
        let mut extra = String::new();
        if t.dim_pct > 0.0 {
            extra += &format!("  dim {:.0}%", t.dim_pct);
        }
        if t.tint_pct >= dim_core::config::MIN_TINT_PCT {
            extra += &format!("  tint {:.0}%", t.tint_pct);
        }
        println!("{h:02}:00  {:>5.0} K  {bar}{extra}", t.kelvin);
    }
}
