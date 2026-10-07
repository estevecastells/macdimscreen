//! What the screen should look like at a given moment.
//!
//! The day has four phases, like f.lux: daylight, a sunset transition that
//! starts at sunset, night, and a morning transition that ends at the wake
//! time (or sunrise if no wake time is set). Colour temperature is
//! interpolated in mireds (1e6 / kelvin), which is roughly perceptually
//! uniform, with smoothstep easing at both ends.

use crate::config::{Config, Mode, NIGHT_SHIFT_OFF_KELVIN};
use crate::solar;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Day,
    Sunset,
    Night,
    Sunrise,
    Paused,
    Off,
    Manual,
}

/// Key times for one local day, as Unix seconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DayPlan {
    pub sunrise: Option<i64>,
    pub sunset: Option<i64>,
    /// Daylight colours are fully restored at this time (wake time, else sunrise).
    pub morning: Option<i64>,
    /// The evening transition starts here (sunset).
    pub evening: Option<i64>,
    /// No sunset and the sun is up: midnight sun.
    pub polar_day: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Target {
    pub phase: Phase,
    /// Colour temperature to show. 6500 K or more means "unchanged".
    pub kelvin: f64,
    /// Overlay dimming in percent.
    pub dim_pct: f64,
    /// Extra-warmth tint in percent; below 25 it isn't shown (see `MIN_TINT_PCT`).
    pub tint_pct: f64,
    /// 0 = full daylight, 1 = full night (before mode overrides).
    pub night_factor: f64,
    /// When the phase next changes, and to what.
    pub next_change: Option<i64>,
    pub next_phase: Option<Phase>,
    pub today: DayPlan,
}

/// Unix time of the local midnight that starts the day containing `now`.
pub fn local_midnight(now: i64, utc_offset: i64) -> i64 {
    now - (now + utc_offset).rem_euclid(86_400)
}

pub fn day_plan(cfg: &Config, midnight: i64) -> DayPlan {
    let sun = solar::sun_times(midnight, cfg.latitude, cfg.longitude);
    let morning = cfg.wake_seconds().map(|w| midnight + w).or(sun.sunrise);
    DayPlan {
        sunrise: sun.sunrise,
        sunset: sun.sunset,
        morning,
        evening: sun.sunset,
        polar_day: sun.sunset.is_none() && sun.up_at_start,
    }
}

/// Where in the day/night cycle `t` falls, ignoring the mode.
fn cycle(plan: &DayPlan, t: i64, transition: i64) -> (f64, Phase, Option<i64>) {
    let (Some(m), Some(s)) = (plan.morning, plan.evening) else {
        return if plan.polar_day { (0.0, Phase::Day, None) } else { (1.0, Phase::Night, None) };
    };
    if m >= s {
        // Wake time after sunset: no daylight window left today.
        return (1.0, Phase::Night, None);
    }
    let tr = transition as f64;
    if t < m - transition {
        (1.0, Phase::Night, Some(m - transition))
    } else if t < m {
        ((m - t) as f64 / tr, Phase::Sunrise, Some(m))
    } else if t < s {
        (0.0, Phase::Day, Some(s))
    } else if t < s + transition {
        ((t - s) as f64 / tr, Phase::Sunset, Some(s + transition))
    } else {
        (1.0, Phase::Night, None) // next change is tomorrow morning
    }
}

fn smoothstep(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Interpolate between two colour temperatures in mired space.
pub fn mix_kelvin(day: f64, night: f64, f: f64) -> f64 {
    let (a, b) = (1e6 / day, 1e6 / night);
    1e6 / (a + (b - a) * f)
}

fn next_phase_after(p: Phase) -> Phase {
    match p {
        Phase::Night => Phase::Sunrise,
        Phase::Sunrise => Phase::Day,
        Phase::Day => Phase::Sunset,
        _ => Phase::Night,
    }
}

pub fn target(cfg: &Config, now: i64, utc_offset: i64) -> Target {
    let midnight = local_midnight(now, utc_offset);
    let today = day_plan(cfg, midnight);
    let transition = (cfg.transition_minutes * 60.0).round() as i64;
    let (raw, phase, mut next_change) = cycle(&today, now, transition);
    if next_change.is_none() && phase == Phase::Night {
        // After this evening's transition: the next change is tomorrow's morning transition.
        let tomorrow = day_plan(cfg, midnight + 86_400);
        next_change = match (tomorrow.morning, tomorrow.evening) {
            (Some(m), Some(s)) if m < s => Some(m - transition),
            _ => None,
        };
    }
    let eased = smoothstep(raw);
    let mut t = Target {
        phase,
        kelvin: mix_kelvin(cfg.day_kelvin, cfg.night_kelvin, eased),
        dim_pct: cfg.night_dim_pct * eased,
        tint_pct: cfg.night_tint_pct * eased,
        night_factor: raw,
        next_phase: next_change.map(|_| next_phase_after(phase)),
        next_change,
        today,
    };
    match cfg.mode {
        Mode::Auto => {}
        Mode::Paused { until } if until > now => {
            (t.phase, t.kelvin, t.dim_pct, t.tint_pct) = (Phase::Paused, NIGHT_SHIFT_OFF_KELVIN, 0.0, 0.0);
            (t.next_change, t.next_phase) = (Some(until), None);
        }
        Mode::Paused { .. } => {} // expired: behaves like Auto until the daemon resets it
        Mode::Off => {
            (t.phase, t.kelvin, t.dim_pct, t.tint_pct) = (Phase::Off, NIGHT_SHIFT_OFF_KELVIN, 0.0, 0.0);
            (t.next_change, t.next_phase) = (None, None);
        }
        Mode::Manual { kelvin, dim_pct, tint_pct } => {
            (t.phase, t.kelvin, t.dim_pct, t.tint_pct) = (Phase::Manual, kelvin, dim_pct, tint_pct);
            (t.next_change, t.next_phase) = (None, None);
        }
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Barcelona, CET (UTC+1), 2026-03-20: sunrise ≈ 06:55, sunset ≈ 19:03.
    const MIDNIGHT: i64 = 1_773_964_800 - 3600;
    const OFFSET: i64 = 3600;

    fn cfg() -> Config {
        Config { latitude: 41.39, longitude: 2.17, location_estimated: false, ..Config::default() }
    }

    fn at(h: i64, m: i64) -> i64 {
        MIDNIGHT + h * 3600 + m * 60
    }

    #[test]
    fn midnight_math() {
        assert_eq!(local_midnight(at(13, 37), OFFSET), MIDNIGHT);
        assert_eq!(local_midnight(MIDNIGHT, OFFSET), MIDNIGHT);
        assert_eq!(local_midnight(MIDNIGHT - 1, OFFSET), MIDNIGHT - 86_400);
    }

    #[test]
    fn follows_the_sun() {
        let c = cfg();
        let noon = target(&c, at(12, 0), OFFSET);
        assert_eq!((noon.phase, noon.kelvin), (Phase::Day, 6500.0));
        assert_eq!(noon.next_phase, Some(Phase::Sunset));
        let sunset = noon.today.sunset.unwrap();
        assert_eq!(noon.next_change, Some(sunset));

        let mid = target(&c, sunset + 20 * 60, OFFSET);
        assert_eq!(mid.phase, Phase::Sunset);
        assert!(mid.kelvin < 6500.0 && mid.kelvin > 3400.0, "{}", mid.kelvin);

        let night = target(&c, at(23, 0), OFFSET);
        assert_eq!(night.phase, Phase::Night);
        assert!((night.kelvin - 3400.0).abs() < 1e-6);
        // Next change: tomorrow's morning transition, 40 minutes before sunrise.
        let tomorrow_sunrise = day_plan(&c, MIDNIGHT + 86_400).sunrise.unwrap();
        assert_eq!(night.next_change, Some(tomorrow_sunrise - 40 * 60));
    }

    #[test]
    fn wake_time_ends_the_night() {
        let c = Config { wake_time: Some("09:00".into()), ..cfg() };
        // After sunrise but before the wake-time transition: still night colours.
        assert_eq!(target(&c, at(8, 0), OFFSET).phase, Phase::Night);
        let ramp = target(&c, at(8, 40), OFFSET);
        assert_eq!(ramp.phase, Phase::Sunrise);
        assert_eq!(ramp.next_change, Some(at(9, 0)));
        assert_eq!(target(&c, at(9, 0), OFFSET).phase, Phase::Day);
    }

    #[test]
    fn transitions_are_monotonic_and_continuous() {
        let c = Config { night_dim_pct: 30.0, night_tint_pct: 40.0, ..cfg() };
        let sunset = target(&c, at(12, 0), OFFSET).today.sunset.unwrap();
        let mut last = target(&c, sunset - 60, OFFSET);
        for s in (0..=50 * 60).step_by(15) {
            let t = target(&c, sunset + s, OFFSET);
            assert!(t.kelvin <= last.kelvin + 1e-9 && t.dim_pct >= last.dim_pct - 1e-9);
            assert!(last.kelvin - t.kelvin < 40.0, "jump of {} K", last.kelvin - t.kelvin);
            last = t;
        }
        assert!((last.dim_pct - 30.0).abs() < 1e-9 && (last.tint_pct - 40.0).abs() < 1e-9);
    }

    #[test]
    fn modes_override_the_schedule() {
        let night = at(23, 0);
        let paused = Config { mode: Mode::Paused { until: night + 3600 }, ..cfg() };
        let t = target(&paused, night, OFFSET);
        assert_eq!((t.phase, t.kelvin, t.next_change), (Phase::Paused, 6500.0, Some(night + 3600)));
        assert_eq!(target(&paused, night + 3600, OFFSET).phase, Phase::Night, "pause expires");

        let off = Config { mode: Mode::Off, ..cfg() };
        assert_eq!(target(&off, night, OFFSET).phase, Phase::Off);

        let manual = Config { mode: Mode::Manual { kelvin: 2700.0, dim_pct: 20.0, tint_pct: 30.0 }, ..cfg() };
        let t = target(&manual, at(12, 0), OFFSET);
        assert_eq!((t.phase, t.kelvin, t.dim_pct, t.tint_pct), (Phase::Manual, 2700.0, 20.0, 30.0));
    }

    #[test]
    fn mired_interpolation() {
        assert_eq!(mix_kelvin(6500.0, 3400.0, 0.0), 6500.0);
        assert!((mix_kelvin(6500.0, 3400.0, 1.0) - 3400.0).abs() < 1e-9);
        // Halfway in mireds is cooler than halfway in kelvin.
        assert!(mix_kelvin(6500.0, 3400.0, 0.5) < 4950.0);
    }

    #[test]
    fn polar_regions_do_not_panic() {
        let c = Config { latitude: 78.2, longitude: 15.6, ..cfg() };
        let june = 1_782_000_000;
        assert_eq!(target(&c, june, 7200).phase, Phase::Day);
        let december = 1_797_811_200;
        assert_eq!(target(&c, december, 3600).phase, Phase::Night);
    }
}
