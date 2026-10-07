//! User configuration, persisted as TOML by the daemon.

use serde::{Deserialize, Serialize};

/// Night Shift's fixed colour-temperature range on Apple Silicon (CoreBrightness
/// `getCCTRange`; `setCCTRange` is refused). Targets outside it are clamped, and
/// anything at or above `NIGHT_SHIFT_OFF_KELVIN` means "Night Shift off".
pub const NIGHT_SHIFT_MIN_KELVIN: f64 = 2700.0;
pub const NIGHT_SHIFT_MAX_KELVIN: f64 = 6000.0;
/// With Night Shift off the display is at its native white (about 6500 K).
pub const NIGHT_SHIFT_OFF_KELVIN: f64 = 6500.0;
/// macOS clamps the Color Tint filter's intensity to at least 25 %.
pub const MIN_TINT_PCT: f64 = 25.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Mode {
    /// Follow the sun (and wake time).
    Auto,
    /// Disabled until `until` (Unix seconds), then back to Auto.
    Paused { until: i64 },
    /// Disabled until turned back on.
    Off,
    /// A fixed colour temperature, dimming and tint, ignoring the schedule.
    Manual {
        kelvin: f64,
        dim_pct: f64,
        #[serde(default)]
        tint_pct: f64,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub mode: Mode,
    pub latitude: f64,
    pub longitude: f64,
    /// True when the location was guessed from the time zone, not set by the user.
    pub location_estimated: bool,
    /// Colour temperature during the day. 6500 K means "no change".
    pub day_kelvin: f64,
    /// Colour temperature at night.
    pub night_kelvin: f64,
    /// Extra dimming at night, in percent (0 = none). Applied by the menu bar app's overlay.
    pub night_dim_pct: f64,
    /// "Extra warmth" at night: Accessibility Color Tint intensity in percent.
    /// 0 = off; otherwise 25–100 (macOS's minimum intensity is 25 %).
    pub night_tint_pct: f64,
    /// How long the evening and morning transitions take.
    pub transition_minutes: f64,
    /// "HH:MM": be back to daylight colours by this time. `None` follows sunrise.
    pub wake_time: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            mode: Mode::Auto,
            latitude: 0.0,
            longitude: 0.0,
            location_estimated: true,
            day_kelvin: 6500.0,
            night_kelvin: 3400.0,
            night_dim_pct: 0.0,
            night_tint_pct: 0.0,
            transition_minutes: 40.0,
            wake_time: None,
        }
    }
}

impl Config {
    pub fn from_toml(s: &str) -> Result<Config, String> {
        let cfg: Config = toml::from_str(s).map_err(|e| e.to_string())?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn to_toml(&self) -> String {
        let body = toml::to_string_pretty(self).expect("config serializes");
        format!(
            "# MacDimScreen configuration. Edit with the menu bar app or `dimctl set`;\n\
             # hand edits are picked up when the daemon restarts.\n\n{body}"
        )
    }

    pub fn validate(&self) -> Result<(), String> {
        let in_range = |v: f64, lo: f64, hi: f64| v.is_finite() && (lo..=hi).contains(&v);
        if !in_range(self.latitude, -90.0, 90.0) {
            return Err(format!("latitude {} must be between -90 and 90", self.latitude));
        }
        if !in_range(self.longitude, -180.0, 180.0) {
            return Err(format!("longitude {} must be between -180 and 180", self.longitude));
        }
        for (name, k) in [("day_kelvin", self.day_kelvin), ("night_kelvin", self.night_kelvin)] {
            if !in_range(k, 1000.0, 10000.0) {
                return Err(format!("{name} {k} must be between 1000 and 10000"));
            }
        }
        if !in_range(self.night_dim_pct, 0.0, 90.0) {
            return Err(format!("night_dim_pct {} must be between 0 and 90", self.night_dim_pct));
        }
        if self.night_tint_pct != 0.0 && !in_range(self.night_tint_pct, MIN_TINT_PCT, 100.0) {
            return Err(format!("night_tint_pct {} must be 0 or between 25 and 100", self.night_tint_pct));
        }
        if !in_range(self.transition_minutes, 1.0, 240.0) {
            return Err(format!("transition_minutes {} must be between 1 and 240", self.transition_minutes));
        }
        if let Some(w) = &self.wake_time {
            parse_hhmm(w).ok_or_else(|| format!("wake_time {w:?} must look like \"07:30\""))?;
        }
        if let Mode::Manual { kelvin, dim_pct, tint_pct } = self.mode {
            if !in_range(kelvin, 1000.0, 10000.0) || !in_range(dim_pct, 0.0, 90.0) || !in_range(tint_pct, 0.0, 100.0) {
                return Err("manual mode needs kelvin in 1000..10000, dim_pct in 0..90 and tint_pct in 0..100".into());
            }
        }
        Ok(())
    }

    /// Wake time as seconds after local midnight.
    pub fn wake_seconds(&self) -> Option<i64> {
        self.wake_time.as_deref().and_then(parse_hhmm)
    }
}

/// "7:05" or "07:05" → seconds after midnight.
pub fn parse_hhmm(s: &str) -> Option<i64> {
    let (h, m) = s.trim().split_once(':')?;
    let (h, m): (i64, i64) = (h.parse().ok()?, m.parse().ok()?);
    ((0..24).contains(&h) && (0..60).contains(&m)).then_some(h * 3600 + m * 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_round_trips_through_toml() {
        let cfg = Config { wake_time: Some("09:00".into()), ..Config::default() };
        assert_eq!(Config::from_toml(&cfg.to_toml()).unwrap(), cfg);
    }

    #[test]
    fn partial_file_uses_defaults() {
        let cfg = Config::from_toml("night_kelvin = 2700\nlatitude = 41.5\n").unwrap();
        assert_eq!(cfg.night_kelvin, 2700.0);
        assert_eq!(cfg.day_kelvin, 6500.0);
        assert_eq!(cfg.mode, Mode::Auto);
    }

    #[test]
    fn rejects_bad_values() {
        assert!(Config::from_toml("latitude = 91").is_err());
        assert!(Config::from_toml("night_dim_pct = 95").is_err());
        assert!(Config::from_toml("night_tint_pct = 10").is_err());
        assert!(Config::from_toml("night_tint_pct = 40").is_ok());
        assert!(Config::from_toml("wake_time = \"25:00\"").is_err());
        assert!(Config::from_toml("typo = 1").is_err());
    }

    #[test]
    fn modes_in_toml() {
        let cfg = Config::from_toml("[mode]\nkind = \"paused\"\nuntil = 1800000000\n").unwrap();
        assert_eq!(cfg.mode, Mode::Paused { until: 1_800_000_000 });
    }

    #[test]
    fn hhmm() {
        assert_eq!(parse_hhmm("09:00"), Some(9 * 3600));
        assert_eq!(parse_hhmm("7:05"), Some(7 * 3600 + 300));
        assert_eq!(parse_hhmm("24:00"), None);
        assert_eq!(parse_hhmm("nine"), None);
    }
}
