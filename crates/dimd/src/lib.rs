//! The MacDimScreen daemon as a library, so `dimctl` and the tests can reuse it.

pub mod client;
pub mod colorfilter;
pub mod flux;
pub mod log;
pub mod nightshift;
pub mod server;

use colorfilter::{ColorFilter, FilterState};
use dim_core::config::{Mode, MIN_TINT_PCT, NIGHT_SHIFT_MAX_KELVIN, NIGHT_SHIFT_MIN_KELVIN};
use dim_core::protocol::Status;
use dim_core::Config;
use nightshift::{NightShift, State};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Targets at or above this are shown with Night Shift off: its warmest-to-
/// coolest range ends at 6000 K, and off (≈6500 K) is the closer match.
const OFF_ABOVE_KELVIN: f64 = 6250.0;
/// Don't re-send temperatures that differ by less than this.
const KELVIN_TOLERANCE: f32 = 10.0;

/// Display settings from before the daemon took over, restored on exit.
/// Kept on disk until restored, so a crash doesn't make us "restore" our own state.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Original {
    pub enabled: bool,
    pub mode: i32,
    pub strength: f32,
    /// The Accessibility colour filter.
    #[serde(default)]
    pub filter: Option<FilterState>,
}

pub type BoxedFilter = Box<dyn ColorFilter + Send>;

pub struct Daemon<N: NightShift> {
    cfg: Config,
    night_shift: Result<N, String>,
    filter: Option<BoxedFilter>,
    /// We switched the colour tint on and haven't switched it off yet.
    tint_on: bool,
    original_path: Option<PathBuf>,
    status: Status,
    started: i64,
    /// Night Shift as last read or set.
    applied: Option<State>,
}

/// Seconds east of UTC for local time at `now` (includes DST).
pub fn utc_offset(now: i64) -> i64 {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&now, &mut tm) };
    tm.tm_gmtoff
}

pub fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Location guess from the time zone when nothing better is known: the zone's
/// standard meridian, at a mid-northern latitude. Shown as "estimated" in the UI.
pub fn estimated_location(now: i64) -> (f64, f64) {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&now, &mut tm) };
    let standard = tm.tm_gmtoff - if tm.tm_isdst > 0 { 3600 } else { 0 };
    (40.0, (standard as f64 / 3600.0 * 15.0).clamp(-180.0, 180.0))
}

impl<N: NightShift> Daemon<N> {
    pub fn new(
        cfg: Config,
        night_shift: Result<N, String>,
        filter: Option<BoxedFilter>,
        original_path: Option<PathBuf>,
        now: i64,
    ) -> Self {
        let status = Status {
            mode: cfg.mode.clone(),
            target: dim_core::target(&cfg, now, utc_offset(now)),
            applied_kelvin: None,
            applied_tint_pct: None,
            clamped: false,
            night_shift_available: night_shift.is_ok(),
            latitude: cfg.latitude,
            longitude: cfg.longitude,
            location_estimated: cfg.location_estimated,
            last_error: night_shift.as_ref().err().cloned(),
            uptime_s: 0.0,
            ticks: 0,
        };
        let mut d =
            Daemon { cfg, night_shift, filter, tint_on: false, original_path, status, started: now, applied: None };
        d.remember_original();
        d
    }

    pub fn config(&self) -> &Config {
        &self.cfg
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    pub fn set_config(&mut self, cfg: Config) -> Result<(), String> {
        cfg.validate()?;
        self.cfg = cfg;
        Ok(())
    }

    /// Save Night Shift's and the colour filter's pre-existing settings, unless a
    /// previous run already did and never got to restore them.
    fn remember_original(&mut self) {
        let (Ok(ns), Some(path)) = (&self.night_shift, &self.original_path) else { return };
        if path.exists() {
            return;
        }
        match ns.read() {
            Ok(s) => {
                let filter = self.filter.as_ref().map(|f| f.read());
                let original = Original { enabled: s.enabled, mode: s.mode, strength: s.strength, filter };
                let json = serde_json::to_string(&original).expect("serializes");
                if let Err(e) = std::fs::write(path, json) {
                    log!("warning: couldn't save display settings to {}: {e}", path.display());
                }
            }
            Err(e) => log!("warning: couldn't read Night Shift settings: {e}"),
        }
    }

    /// Put Night Shift and the colour filter back the way they were before the daemon started.
    pub fn restore(&mut self) -> Result<(), String> {
        let (Ok(ns), Some(path)) = (&self.night_shift, &self.original_path) else { return Ok(()) };
        let Ok(json) = std::fs::read_to_string(path) else { return Ok(()) };
        let original: Original = serde_json::from_str(&json).map_err(|e| e.to_string())?;
        if let (Some(f), Some(saved)) = (&self.filter, original.filter) {
            f.write(saved);
        }
        ns.set_strength(original.strength)?;
        ns.set_mode(original.mode)?;
        ns.set_enabled(original.enabled)?;
        let _ = std::fs::remove_file(path);
        Ok(())
    }

    /// Show the extra-warmth tint at `pct` percent, or switch it off. The filter
    /// is only touched while the feature is in use, so a colour filter the user
    /// set up themselves is left alone.
    fn apply_tint(&mut self, pct: f64) -> Option<f64> {
        let f = self.filter.as_ref()?;
        if pct >= MIN_TINT_PCT {
            let want = FilterState {
                enabled: true,
                kind: colorfilter::TYPE_COLOR_TINT,
                hue: colorfilter::TINT_HUE,
                intensity: (pct / 100.0).min(1.0),
            };
            f.write(want);
            self.tint_on = true;
            Some(pct)
        } else {
            if self.tint_on {
                f.write(FilterState { enabled: false, ..f.read() });
                self.tint_on = false;
            }
            None
        }
    }

    /// Compute the target for `now` and make Night Shift match it.
    pub fn tick(&mut self, now: i64) -> &Status {
        if let Mode::Paused { until } = self.cfg.mode {
            if until <= now {
                self.cfg.mode = Mode::Auto;
            }
        }
        let target = dim_core::target(&self.cfg, now, utc_offset(now));
        let wanted = (target.kelvin < OFF_ABOVE_KELVIN)
            .then(|| target.kelvin.clamp(NIGHT_SHIFT_MIN_KELVIN, NIGHT_SHIFT_MAX_KELVIN) as f32);

        let result = match &self.night_shift {
            Ok(ns) => apply(ns, wanted),
            Err(e) => Err(e.clone()),
        };
        match result {
            Ok(state) => {
                self.applied = Some(state);
                self.status.last_error = None;
            }
            Err(e) => {
                if self.status.last_error.as_ref() != Some(&e) {
                    log!("error: {e}");
                }
                self.status.last_error = Some(e);
            }
        }

        let tint = self.apply_tint(target.tint_pct);

        let s = &mut self.status;
        s.applied_tint_pct = tint;
        s.mode = self.cfg.mode.clone();
        s.clamped = target.kelvin < NIGHT_SHIFT_MIN_KELVIN;
        s.target = target;
        s.applied_kelvin = self.applied.filter(|a| a.enabled).map(|a| a.kelvin.round() as f64);
        s.latitude = self.cfg.latitude;
        s.longitude = self.cfg.longitude;
        s.location_estimated = self.cfg.location_estimated;
        s.uptime_s = (now - self.started) as f64;
        s.ticks += 1;
        &self.status
    }
}

/// Make Night Shift show `wanted` (None = off), changing only what differs, so
/// it also re-asserts our setting if the user or macOS changed it.
fn apply<N: NightShift>(ns: &N, wanted: Option<f32>) -> Result<State, String> {
    let mut s = ns.read()?;
    match wanted {
        None => {
            if s.enabled {
                ns.set_enabled(false)?;
                s.enabled = false;
            }
        }
        Some(k) => {
            // Manual mode, so Night Shift's own schedule doesn't switch it off at sunrise.
            // Changing the mode also switches Night Shift off, so read it again.
            if s.mode != 0 {
                ns.set_mode(0)?;
                s = ns.read()?;
            }
            if (s.kelvin - k).abs() >= KELVIN_TOLERANCE {
                ns.set_kelvin(k)?;
                s.kelvin = k;
            }
            if !s.enabled {
                ns.set_enabled(true)?;
                s.enabled = true;
            }
        }
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Fake {
        state: RefCell<Option<State>>,
        calls: RefCell<Vec<String>>,
    }

    impl Fake {
        fn new(enabled: bool, mode: i32, kelvin: f32) -> Self {
            Fake { state: RefCell::new(Some(State { enabled, mode, kelvin, strength: 0.5 })), ..Default::default() }
        }
        fn calls(&self) -> Vec<String> {
            std::mem::take(&mut self.calls.borrow_mut())
        }
        fn with(&self, f: impl FnOnce(&mut State), call: String) -> Result<(), String> {
            self.calls.borrow_mut().push(call);
            f(self.state.borrow_mut().as_mut().unwrap());
            Ok(())
        }
    }

    impl NightShift for &Fake {
        fn read(&self) -> Result<State, String> {
            self.state.borrow().ok_or_else(|| "unavailable".to_string())
        }
        fn set_enabled(&self, on: bool) -> Result<(), String> {
            self.with(|s| s.enabled = on, format!("enabled {on}"))
        }
        fn set_mode(&self, mode: i32) -> Result<(), String> {
            // Like the real thing: changing the schedule mode switches Night Shift off.
            self.with(|s| (s.mode, s.enabled) = (mode, false), format!("mode {mode}"))
        }
        fn set_kelvin(&self, kelvin: f32) -> Result<(), String> {
            self.with(|s| s.kelvin = kelvin, format!("kelvin {kelvin}"))
        }
        fn set_strength(&self, strength: f32) -> Result<(), String> {
            self.with(|s| s.strength = strength, format!("strength {strength}"))
        }
    }

    fn night_cfg(mode: Mode) -> Config {
        // Manual mode makes the target independent of the time of day.
        Config { mode, latitude: 41.5, longitude: 2.4, ..Config::default() }
    }

    #[test]
    fn applies_and_then_stays_quiet() {
        let fake = Fake::new(false, 1, 6000.0);
        let mut d = Daemon::new(
            night_cfg(Mode::Manual { kelvin: 3400.0, dim_pct: 0.0, tint_pct: 0.0 }),
            Ok(&fake),
            None,
            None,
            0,
        );
        d.tick(0);
        assert_eq!(fake.calls(), ["mode 0", "kelvin 3400", "enabled true"]);
        assert_eq!(d.status().applied_kelvin, Some(3400.0));
        d.tick(15);
        assert!(fake.calls().is_empty(), "no writes when nothing changed");
    }

    #[test]
    fn leaves_night_shift_schedule_and_stays_on() {
        let fake = Fake::new(true, 1, 3400.0);
        let mut d = Daemon::new(
            night_cfg(Mode::Manual { kelvin: 3400.0, dim_pct: 0.0, tint_pct: 0.0 }),
            Ok(&fake),
            None,
            None,
            0,
        );
        d.tick(0);
        assert_eq!(fake.calls(), ["mode 0", "enabled true"]);
        assert_eq!(d.status().applied_kelvin, Some(3400.0));
    }

    #[test]
    fn reasserts_after_outside_changes() {
        let fake = Fake::new(true, 0, 3400.0);
        let mut d = Daemon::new(
            night_cfg(Mode::Manual { kelvin: 3400.0, dim_pct: 0.0, tint_pct: 0.0 }),
            Ok(&fake),
            None,
            None,
            0,
        );
        d.tick(0);
        (&fake).set_enabled(false).unwrap();
        fake.calls();
        d.tick(15);
        assert_eq!(fake.calls(), ["enabled true"]);
    }

    #[test]
    fn off_and_clamping() {
        let fake = Fake::new(true, 0, 3400.0);
        let mut d = Daemon::new(night_cfg(Mode::Off), Ok(&fake), None, None, 0);
        d.tick(0);
        assert_eq!(fake.calls(), ["enabled false"]);
        assert_eq!(d.status().applied_kelvin, None);

        d.set_config(night_cfg(Mode::Manual { kelvin: 1900.0, dim_pct: 0.0, tint_pct: 0.0 })).unwrap();
        d.tick(15);
        assert_eq!(fake.calls(), ["kelvin 2700", "enabled true"]);
        assert!(d.status().clamped);
    }

    #[test]
    fn pause_expires_back_to_auto() {
        let fake = Fake::new(false, 0, 6000.0);
        let mut d = Daemon::new(night_cfg(Mode::Paused { until: 100 }), Ok(&fake), None, None, 0);
        d.tick(50);
        assert_eq!(d.config().mode, Mode::Paused { until: 100 });
        d.tick(100);
        assert_eq!(d.config().mode, Mode::Auto);
    }

    #[test]
    fn restores_original_settings_once() {
        let dir = std::env::temp_dir().join(format!("dimd-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("original.json");
        let _ = std::fs::remove_file(&path);

        let fake = Fake::new(false, 1, 6000.0);
        let mut d = Daemon::new(
            night_cfg(Mode::Manual { kelvin: 3000.0, dim_pct: 0.0, tint_pct: 0.0 }),
            Ok(&fake),
            None,
            Some(path.clone()),
            0,
        );
        d.tick(0);
        // A restarted daemon must not overwrite the saved original with our own state.
        let _second = Daemon::new(night_cfg(Mode::Auto), Ok(&fake), None, Some(path.clone()), 0);
        fake.calls();
        d.restore().unwrap();
        assert_eq!(fake.calls(), ["strength 0.5", "mode 1", "enabled false"]);
        assert!(!path.exists());
        d.restore().unwrap();
        assert!(fake.calls().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[derive(Clone)]
    struct FakeFilter(std::sync::Arc<std::sync::Mutex<FilterState>>);

    impl ColorFilter for FakeFilter {
        fn read(&self) -> FilterState {
            *self.0.lock().unwrap()
        }
        fn write(&self, s: FilterState) {
            *self.0.lock().unwrap() = s;
        }
    }

    #[test]
    fn tint_follows_target_and_leaves_user_filters_alone() {
        // The user has their own grayscale filter (type 1), switched off.
        let user = FilterState { enabled: false, kind: 1, hue: 0.5, intensity: 0.8 };
        let filter = FakeFilter(std::sync::Arc::new(std::sync::Mutex::new(user)));
        let fake = Fake::new(false, 0, 6000.0);
        let manual = |tint_pct| night_cfg(Mode::Manual { kelvin: 3400.0, dim_pct: 0.0, tint_pct });

        // Feature off: never touched.
        let mut d = Daemon::new(manual(0.0), Ok(&fake), Some(Box::new(filter.clone())), None, 0);
        d.tick(0);
        assert_eq!(filter.read(), user);

        // Below macOS's 25 % minimum: still off.
        d.set_config(manual(20.0)).unwrap();
        assert_eq!(d.tick(15).applied_tint_pct, None);
        assert_eq!(filter.read(), user);

        d.set_config(manual(40.0)).unwrap();
        assert_eq!(d.tick(30).applied_tint_pct, Some(40.0));
        let on = filter.read();
        assert!(on.enabled && on.kind == colorfilter::TYPE_COLOR_TINT && (on.intensity - 0.4).abs() < 1e-9);

        d.set_config(manual(0.0)).unwrap();
        d.tick(45);
        assert!(!filter.read().enabled, "switched off again once the feature stops");
    }

    #[test]
    fn restores_the_users_color_filter() {
        let dir = std::env::temp_dir().join(format!("dimd-filter-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("original.json");
        let _ = std::fs::remove_file(&path);
        let user = FilterState { enabled: true, kind: 1, hue: 0.5, intensity: 0.8 };
        let filter = FakeFilter(std::sync::Arc::new(std::sync::Mutex::new(user)));
        let fake = Fake::new(false, 0, 6000.0);
        let cfg = night_cfg(Mode::Manual { kelvin: 3400.0, dim_pct: 0.0, tint_pct: 50.0 });
        let mut d = Daemon::new(cfg, Ok(&fake), Some(Box::new(filter.clone())), Some(path.clone()), 0);
        d.tick(0);
        assert_eq!(filter.read().kind, colorfilter::TYPE_COLOR_TINT);
        d.restore().unwrap();
        assert_eq!(filter.read(), user);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_night_shift_is_reported() {
        let mut d: Daemon<&Fake> = Daemon::new(night_cfg(Mode::Auto), Err("no Night Shift".into()), None, None, 0);
        let s = d.tick(0);
        assert!(!s.night_shift_available);
        assert_eq!(s.last_error.as_deref(), Some("no Night Shift"));
    }
}
