//! Paths for Weather, re-exporting `nyx_core::paths` and adding the SQLite cache file.

pub use nyx_core::paths::*;

use std::path::PathBuf;

/// `~/.cache/nyx-weather/weather.db`
pub fn cache_db() -> PathBuf {
    cache_dir().join("weather.db")
}
