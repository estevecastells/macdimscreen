//! Hardware-independent core of MacDimScreen: sun position, the day/night
//! schedule, configuration and the daemon's wire protocol. No OS calls, so all
//! of it is unit-testable.

pub mod config;
pub mod protocol;
pub mod schedule;
pub mod solar;

pub use config::{Config, Mode};
pub use schedule::{target, Phase, Target};
