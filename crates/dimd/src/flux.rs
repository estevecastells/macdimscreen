//! First-run import of f.lux settings, so switching over keeps your location,
//! night colour and wake time.

use dim_core::Config;
use std::process::Command;

fn read(key: &str) -> Option<String> {
    let out = Command::new("/usr/bin/defaults").args(["read", "org.herf.Flux", key]).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Apply whatever f.lux settings exist on top of `cfg`. Returns what was imported.
pub fn import(cfg: &mut Config) -> Vec<String> {
    let mut imported = Vec::new();
    if let Some((lat, lon)) = read("location").and_then(|s| {
        let (a, b) = s.split_once(',')?;
        Some((a.trim().parse::<f64>().ok()?, b.trim().parse::<f64>().ok()?))
    }) {
        if (-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon) && (lat, lon) != (0.0, 0.0) {
            (cfg.latitude, cfg.longitude, cfg.location_estimated) = (lat, lon, false);
            imported.push(format!("location {lat},{lon}"));
        }
    }
    if let Some(k) =
        read("nightColorTemp").and_then(|s| s.parse::<f64>().ok()).filter(|k| (1000.0..=10000.0).contains(k))
    {
        cfg.night_kelvin = k;
        imported.push(format!("night {k} K"));
    }
    // f.lux stores the wake time as minutes after midnight.
    if let Some(m) = read("wakeTime").and_then(|s| s.parse::<i64>().ok()).filter(|m| (0..1440).contains(m)) {
        cfg.wake_time = Some(format!("{:02}:{:02}", m / 60, m % 60));
        imported.push(format!("wake time {:02}:{:02}", m / 60, m % 60));
    }
    imported
}
