//! Unix-socket server: newline-delimited JSON requests/responses.
//!
//! The socket lives in the user's Library with mode 0600, and connections from
//! any other uid are dropped.

use crate::nightshift::NightShift;
use crate::Daemon;
use dim_core::protocol::{Request, Response, PROTOCOL_VERSION};
use dim_core::Config;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

const MAX_LINE: usize = 64 * 1024;

pub struct Shared<N: NightShift> {
    daemon: Mutex<Daemon<N>>,
    kick: (Mutex<bool>, Condvar),
    config_path: Option<PathBuf>,
}

impl<N: NightShift> Shared<N> {
    pub fn new(daemon: Daemon<N>, config_path: Option<PathBuf>) -> Arc<Self> {
        Arc::new(Shared { daemon: Mutex::new(daemon), kick: (Mutex::new(false), Condvar::new()), config_path })
    }

    /// Lock the daemon, recovering from a poisoned mutex (a panicked thread
    /// must not prevent us from restoring Night Shift).
    pub fn daemon(&self) -> MutexGuard<'_, Daemon<N>> {
        self.daemon.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Wake the control loop early (e.g. after a settings change).
    pub fn kick(&self) {
        let (m, cv) = &self.kick;
        *m.lock().unwrap_or_else(|e| e.into_inner()) = true;
        cv.notify_all();
    }

    /// Sleep up to `timeout`. Returns true (early) if kicked.
    pub fn wait(&self, timeout: Duration) -> bool {
        let (m, cv) = &self.kick;
        let guard = m.lock().unwrap_or_else(|e| e.into_inner());
        let (mut guard, _) =
            cv.wait_timeout_while(guard, timeout, |kicked| !*kicked).unwrap_or_else(|e| e.into_inner());
        std::mem::take(&mut *guard)
    }

    pub fn handle(&self, req: Request) -> Response {
        match req {
            Request::Ping => Response::Pong { version: env!("CARGO_PKG_VERSION").into(), protocol: PROTOCOL_VERSION },
            Request::Status => Response::Status { status: self.daemon().status().clone() },
            Request::GetConfig => Response::Config { config: self.daemon().config().clone() },
            Request::SetMode { mode } => self.update(|c| c.mode = mode),
            Request::SetConfig { config } => self.update(move |c| *c = *config),
        }
    }

    fn update(&self, f: impl FnOnce(&mut Config)) -> Response {
        let mut daemon = self.daemon();
        let mut cfg = daemon.config().clone();
        f(&mut cfg);
        if let Err(e) = daemon.set_config(cfg.clone()) {
            return Response::Error { message: e };
        }
        // Apply now, so the reply and the next status already reflect the change.
        daemon.tick(crate::unix_now());
        drop(daemon);
        if let Some(path) = &self.config_path {
            if let Err(e) = save_config(path, &cfg) {
                crate::log!("warning: could not persist config to {}: {e}", path.display());
            }
        }
        crate::log!("config updated: {cfg:?}");
        self.kick();
        Response::Config { config: cfg }
    }

    pub fn config_path(&self) -> Option<&Path> {
        self.config_path.as_deref()
    }
}

pub fn save_config(path: &Path, cfg: &Config) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, cfg.to_toml())?;
    std::fs::rename(tmp, path)
}

fn peer_uid(stream: &UnixStream) -> Option<u32> {
    let (mut uid, mut gid) = (0, 0);
    let r = unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) };
    (r == 0).then_some(uid)
}

fn serve_connection<N: NightShift>(shared: &Shared<N>, stream: UnixStream) {
    let my_uid = unsafe { libc::geteuid() };
    if peer_uid(&stream) != Some(my_uid) {
        return;
    }
    let _ = stream.set_read_timeout(Some(Duration::from_secs(60)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let Ok(mut writer) = stream.try_clone() else { return };
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        match Read::by_ref(&mut reader).take(MAX_LINE as u64).read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        if !line.ends_with('\n') && line.len() >= MAX_LINE {
            let _ = writeln!(writer, "{}", json(&Response::Error { message: "request too large".into() }));
            return;
        }
        let resp = match serde_json::from_str::<Request>(line.trim()) {
            Ok(req) => shared.handle(req),
            Err(e) => Response::Error { message: format!("bad request: {e}") },
        };
        if writeln!(writer, "{}", json(&resp)).is_err() {
            return;
        }
    }
}

fn json(r: &Response) -> String {
    serde_json::to_string(r).unwrap_or_else(|e| format!(r#"{{"type":"error","message":"{e}"}}"#))
}

/// Bind the socket (replacing a stale one) and serve forever on background threads.
pub fn spawn<N: NightShift + Send + 'static>(shared: Arc<Shared<N>>, path: &Path) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    std::thread::Builder::new().name("socket-accept".into()).spawn(move || {
        for stream in listener.incoming().flatten() {
            let shared = shared.clone();
            let _ =
                std::thread::Builder::new().name("socket-conn".into()).spawn(move || serve_connection(&shared, stream));
        }
    })?;
    Ok(())
}
