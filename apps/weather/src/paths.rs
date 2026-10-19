//! Paths for Weather, re-exporting `nixe_core::paths` and adding the SQLite cache file.

pub use nixe_core::paths::*;

use std::path::PathBuf;

/// `~/.cache/nixe-weather/weather.db`
pub fn cache_db() -> PathBuf {
    cache_dir().join("weather.db")
}
