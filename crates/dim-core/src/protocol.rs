//! Wire protocol between the daemon and its clients (CLI, Swift app).
//!
//! Transport: a Unix domain socket carrying newline-delimited JSON. Each
//! request line gets exactly one response line.

use crate::config::{Config, Mode};
use crate::schedule::Target;
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;

/// `~/Library/Application Support/MacDimScreen/dimd.sock`, or `$MACDIMSCREEN_SOCKET`.
pub fn default_socket_path() -> std::path::PathBuf {
    if let Some(p) = std::env::var_os("MACDIMSCREEN_SOCKET") {
        return p.into();
    }
    support_dir().join("dimd.sock")
}

/// `~/Library/Application Support/MacDimScreen`.
pub fn support_dir() -> std::path::PathBuf {
    let home = std::env::var_os("HOME").unwrap_or_else(|| "/tmp".into());
    std::path::Path::new(&home).join("Library/Application Support/MacDimScreen")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    Ping,
    Status,
    GetConfig,
    SetMode { mode: Mode },
    SetConfig { config: Box<Config> },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Pong { version: String, protocol: u32 },
    Status { status: Status },
    Config { config: Config },
    Error { message: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Status {
    pub mode: Mode,
    /// What the schedule asks for right now.
    pub target: Target,
    /// Colour temperature Night Shift is actually showing; `None` when it's off.
    pub applied_kelvin: Option<f64>,
    /// Extra-warmth tint currently shown, in percent; `None` when off.
    pub applied_tint_pct: Option<f64>,
    /// The target is warmer than Night Shift can go (2700 K), so it was clamped.
    pub clamped: bool,
    /// False if this Mac has no Night Shift (CoreBrightness unavailable).
    pub night_shift_available: bool,
    pub latitude: f64,
    pub longitude: f64,
    pub location_estimated: bool,
    pub last_error: Option<String>,
    /// Seconds since the daemon started.
    pub uptime_s: f64,
    pub ticks: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_wire_format() {
        let r: Request =
            serde_json::from_str(r#"{"cmd":"set_mode","mode":{"kind":"paused","until":1800000000}}"#).unwrap();
        assert_eq!(r, Request::SetMode { mode: Mode::Paused { until: 1_800_000_000 } });
        assert_eq!(serde_json::to_string(&Request::Status).unwrap(), r#"{"cmd":"status"}"#);
    }

    /// Shared with the Swift app's checks (app/Sources/KitChecks).
    #[test]
    fn shared_fixtures_parse() {
        let fixture = include_str!("../../../fixtures/status_response.json");
        let Response::Status { status } = serde_json::from_str(fixture).unwrap() else { panic!("expected status") };
        assert_eq!(status.target.phase, crate::Phase::Night);
        assert_eq!(status.applied_kelvin, Some(3400.0));
        // Round-trips losslessly, so the fixture covers every field the daemon emits.
        let again: Status = serde_json::from_value(serde_json::to_value(&status).unwrap()).unwrap();
        assert_eq!(again, status);
        let fields = serde_json::to_value(&status).unwrap().as_object().unwrap().len();
        let raw: serde_json::Value = serde_json::from_str(fixture).unwrap();
        assert_eq!(fields, raw["status"].as_object().unwrap().len(), "fixture is missing Status fields");

        // Same order as the Swift checks encode them.
        let expected = [
            Request::Status,
            Request::GetConfig,
            Request::SetMode { mode: Mode::Auto },
            Request::SetMode { mode: Mode::Off },
            Request::SetMode { mode: Mode::Paused { until: 1_800_000_000 } },
            Request::SetMode { mode: Mode::Manual { kelvin: 2700.0, dim_pct: 20.0, tint_pct: 40.0 } },
        ];
        let parsed: Vec<Request> = include_str!("../../../fixtures/requests.jsonl")
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(parsed, expected);

        let config: Response = serde_json::from_str(include_str!("../../../fixtures/config_response.json")).unwrap();
        let Response::Config { config } = config else { panic!("expected config") };
        config.validate().unwrap();
        assert_eq!(config.wake_time.as_deref(), Some("09:00"));
    }

    #[test]
    fn response_wire_format() {
        let r = Response::Error { message: "nope".into() };
        assert_eq!(serde_json::to_string(&r).unwrap(), r#"{"type":"error","message":"nope"}"#);
    }
}
