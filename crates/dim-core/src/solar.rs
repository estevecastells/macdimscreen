//! Sun position and sunrise/sunset times.
//!
//! Low-precision solar ephemeris (Astronomical Almanac / NOAA style), good to
//! about 0.01° in elevation, which is about a minute at sunrise. That is far
//! more precise than a screen-warmth schedule needs.

const DEG: f64 = std::f64::consts::PI / 180.0;

/// Elevation at which the sun's upper limb touches the horizon, accounting for
/// atmospheric refraction. The standard definition of sunrise and sunset.
pub const HORIZON_DEG: f64 = -0.833;

/// Sun elevation above the horizon in degrees at a Unix time.
pub fn elevation_deg(unix: f64, latitude: f64, longitude: f64) -> f64 {
    // Days since J2000.0.
    let n = unix / 86_400.0 + 2_440_587.5 - 2_451_545.0;
    let mean_longitude = (280.460 + 0.985_647_4 * n).rem_euclid(360.0);
    let anomaly = ((357.528 + 0.985_600_3 * n).rem_euclid(360.0)) * DEG;
    let ecliptic_longitude = (mean_longitude + 1.915 * anomaly.sin() + 0.020 * (2.0 * anomaly).sin()) * DEG;
    let obliquity = (23.439 - 0.000_000_4 * n) * DEG;

    let right_ascension = (obliquity.cos() * ecliptic_longitude.sin()).atan2(ecliptic_longitude.cos());
    let declination = (obliquity.sin() * ecliptic_longitude.sin()).asin();

    let gmst_hours = (18.697_374_558 + 24.065_709_824_419_08 * n).rem_euclid(24.0);
    let hour_angle = (gmst_hours * 15.0 + longitude) * DEG - right_ascension;

    let lat = latitude * DEG;
    let sin_elevation = lat.sin() * declination.sin() + lat.cos() * declination.cos() * hour_angle.cos();
    sin_elevation.clamp(-1.0, 1.0).asin() / DEG
}

/// Sunrise and sunset within a 24-hour window, as Unix seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SunTimes {
    /// First time the sun rises in the window. `None` if it doesn't (polar night or polar day).
    pub sunrise: Option<i64>,
    /// First time the sun sets after `sunrise` (or in the window, if it doesn't rise).
    pub sunset: Option<i64>,
    /// Whether the sun is up at the start of the window.
    pub up_at_start: bool,
}

/// Find sunrise and sunset in `[start, start + 24h)`, typically a local day.
pub fn sun_times(start: i64, latitude: f64, longitude: f64) -> SunTimes {
    const STEP: i64 = 600;
    let above = |t: i64| elevation_deg(t as f64, latitude, longitude) - HORIZON_DEG;

    let up_at_start = above(start) > 0.0;
    let mut sunrise = None;
    let mut sunset = None;
    let mut t = start;
    let mut prev = above(t);
    while t < start + 86_400 {
        let next_t = (t + STEP).min(start + 86_400);
        let next = above(next_t);
        if prev <= 0.0 && next > 0.0 && sunrise.is_none() {
            sunrise = Some(bisect(t, next_t, &above));
        } else if prev > 0.0 && next <= 0.0 && sunset.is_none_or(|s| sunrise.is_some_and(|r| s < r)) {
            sunset = Some(bisect(t, next_t, &above));
        }
        prev = next;
        t = next_t;
    }
    SunTimes { sunrise, sunset, up_at_start }
}

/// Second-accurate crossing of zero between `lo` and `hi` (signs differ).
fn bisect(mut lo: i64, mut hi: i64, f: &impl Fn(i64) -> f64) -> i64 {
    let lo_sign = f(lo) > 0.0;
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if (f(mid) > 0.0) == lo_sign {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    hi
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-06-21 00:00 UTC.
    const SOLSTICE_UTC: i64 = 1_782_000_000;

    fn hm(unix: i64, offset_h: i64) -> (i64, i64) {
        let local = (unix + offset_h * 3600).rem_euclid(86_400);
        (local / 3600, local % 3600 / 60)
    }

    #[test]
    fn barcelona_equinox() {
        // 2026-03-20, Barcelona (41.39 N, 2.17 E), UTC+1. Solar noon is 12:58.7 (longitude −8.7 min,
        // equation of time +7.4 min) and the equinox day lasts about 12 h 08 m: 06:55 to 19:03.
        let day = 1_773_964_800 - 3600; // 2026-03-20 00:00 local
        let s = sun_times(day, 41.39, 2.17);
        let (rh, rm) = hm(s.sunrise.unwrap(), 1);
        let (sh, sm) = hm(s.sunset.unwrap(), 1);
        assert!((rh * 60 + rm - (6 * 60 + 55)).abs() <= 3, "sunrise {rh}:{rm}");
        assert!((sh * 60 + sm - (19 * 60 + 3)).abs() <= 3, "sunset {sh}:{sm}");
        assert!(!s.up_at_start);
    }

    #[test]
    fn noon_is_high_and_midnight_is_low() {
        let noon_utc = 1_773_964_800 + 12 * 3600; // 2026-03-20 12:00 UTC
        assert!(elevation_deg(noon_utc as f64, 0.0, 0.0) > 85.0);
        assert!(elevation_deg((noon_utc + 12 * 3600) as f64, 0.0, 0.0) < -85.0);
    }

    #[test]
    fn polar_day_and_night() {
        let day = SOLSTICE_UTC;
        let june_svalbard = sun_times(day, 78.2, 15.6);
        assert!(june_svalbard.up_at_start && june_svalbard.sunrise.is_none() && june_svalbard.sunset.is_none());
        let june_antarctica = sun_times(day, -78.0, 0.0);
        assert!(!june_antarctica.up_at_start && june_antarctica.sunrise.is_none());
    }
}
