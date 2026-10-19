//! Preferences for Weather (`~/.config/nixe-weather/settings.toml`).

pub use nixe_theme::ThemeMode;

use nixe_weather_core::{
    Location, OpenMeteoConfig, PrecipitationUnit, TemperatureUnit, UnitPrefs, WindSpeedUnit,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub mode: ThemeMode,
    pub theme: String,
    pub glow: bool,
    pub reduce_motion: bool,
    pub sidebar_collapsed: bool,
    pub last_section: String,
    /// When true, adds a Hyprland floating window rule (`480×740`) and floats the window.
    pub float_window: bool,
    pub temperature_unit: TemperatureUnit,
    pub wind_speed_unit: WindSpeedUnit,
    pub precipitation_unit: PrecipitationUnit,
    pub use_24h_time: bool,
    /// Default layer on the Radar & Overlays page (`"radar"`, `"satellite"`, `"wind"`, `"temp"`).
    pub default_radar_layer: String,
    /// Radar overlay opacity (`40..=100` percent).
    pub radar_opacity_pct: u8,
    /// Forecast refresh interval in minutes (15, 30, or 60).
    pub refresh_interval_mins: u32,
    /// Active location ID (`"{lat:.4},{lon:.4}"`).
    pub active_location_id: String,
    /// Saved locations in the user's picker.
    pub saved_locations: Vec<Location>,
    /// Fetch active US severe-weather alerts from NOAA/NWS.
    pub us_noaa_alerts: bool,
    /// Whether this installation is used commercially (requires API key or self-hosted URL).
    pub commercial_use: bool,
    /// Optional Open-Meteo commercial API key.
    pub open_meteo_api_key: String,
    /// Optional custom Open-Meteo forecast endpoint URL.
    pub custom_forecast_url: String,
    /// Optional custom Open-Meteo air-quality endpoint URL.
    pub custom_air_quality_url: String,
    /// Optional custom Open-Meteo geocoding endpoint URL.
    pub custom_geocoding_url: String,
}

impl Default for Prefs {
    fn default() -> Self {
        let default_loc = Location::new(
            "New York",
            Some("New York".into()),
            Some("United States".into()),
            Some("US".into()),
            40.7143,
            -74.0060,
            Some("America/New_York".into()),
            Some(10.0),
        );
        Self {
            mode: ThemeMode::Omarchy,
            theme: "tokyo-night".into(),
            glow: true,
            reduce_motion: false,
            sidebar_collapsed: false,
            last_section: "overview".into(),
            float_window: false,
            temperature_unit: TemperatureUnit::Fahrenheit,
            wind_speed_unit: WindSpeedUnit::Mph,
            precipitation_unit: PrecipitationUnit::Inches,
            use_24h_time: false,
            default_radar_layer: "radar".into(),
            radar_opacity_pct: 80,
            refresh_interval_mins: 30,
            active_location_id: default_loc.id.clone(),
            saved_locations: vec![default_loc],
            us_noaa_alerts: true,
            commercial_use: false,
            open_meteo_api_key: String::new(),
            custom_forecast_url: String::new(),
            custom_air_quality_url: String::new(),
            custom_geocoding_url: String::new(),
        }
    }
}

impl Prefs {
    pub fn units(&self) -> UnitPrefs {
        UnitPrefs {
            temperature: self.temperature_unit,
            wind_speed: self.wind_speed_unit,
            precipitation: self.precipitation_unit,
            use_24h_time: self.use_24h_time,
        }
    }

    pub fn active_location(&self) -> Option<Location> {
        self.saved_locations
            .iter()
            .find(|l| l.id == self.active_location_id)
            .or_else(|| self.saved_locations.first())
            .cloned()
    }

    pub fn open_meteo_config(&self) -> OpenMeteoConfig {
        let mut cfg = OpenMeteoConfig::default();
        if !self.custom_forecast_url.trim().is_empty() {
            cfg.forecast_url = self.custom_forecast_url.trim().to_string();
        }
        if !self.custom_air_quality_url.trim().is_empty() {
            cfg.air_quality_url = self.custom_air_quality_url.trim().to_string();
        }
        if !self.custom_geocoding_url.trim().is_empty() {
            cfg.geocoding_url = self.custom_geocoding_url.trim().to_string();
        }
        if !self.open_meteo_api_key.trim().is_empty() {
            cfg.api_key = Some(self.open_meteo_api_key.trim().to_string());
        }
        cfg.commercial_use = self.commercial_use;
        cfg.fetch_us_noaa_alerts = self.us_noaa_alerts;
        cfg
    }
}

/// Apply the floating-window preference to `~/.config/hypr/nixe.lua` and the live Hyprland session.
pub fn apply_floating_window(float: bool) {
    let hypr_dir = crate::paths::home().join(".config/hypr");
    let nixe_lua = hypr_dir.join("nixe.lua");
    let rule_line =
        r#"o.window("^io\\.github\\.design_nexus\\.nixe\\.Weather$", { float = true, center = true, size = { 460, 720 } })"#;

    if hypr_dir.is_dir() {
        let existing = std::fs::read_to_string(&nixe_lua).unwrap_or_else(|_| {
            "-- Written by the Nixe installer: Nixe apps that open as floating windows.\n-- Reinstalling an app rewrites this file.\n".to_string()
        });
        let mut lines: Vec<&str> = existing
            .lines()
            .filter(|l| !l.contains("nixe\\.Weather") && !l.contains("nixe.Weather"))
            .collect();
        if float {
            lines.push(rule_line);
        }
        let mut out = lines.join("\n");
        if !out.ends_with('\n') {
            out.push('\n');
        }
        let _ = crate::cmd::atomic_write(&nixe_lua, &out);
    }

    std::thread::spawn(move || {
        let _ = std::process::Command::new("hyprctl").arg("reload").output();

        // Find all running Weather windows from `hyprctl clients -j`
        let clients_out = std::process::Command::new("hyprctl")
            .args(["clients", "-j"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());

        if let Some(arr) = clients_out.as_ref().and_then(|v| v.as_array()) {
            for c in arr {
                let class = c.get("class").and_then(|v| v.as_str()).unwrap_or("");
                if class != "io.github.design_nexus.nixe.Weather" {
                    continue;
                }
                let Some(addr) = c.get("address").and_then(|v| v.as_str()) else {
                    continue;
                };
                let is_floating = c.get("floating").and_then(|v| v.as_bool()).unwrap_or(false);
                let win_sel = format!("address:{addr}");

                if is_floating != float {
                    let lua_toggle = format!(
                        r#"hl.dsp.window.float({{ window = "{win_sel}", action = "toggle" }})"#
                    );
                    let ok = std::process::Command::new("hyprctl")
                        .args(["dispatch", &lua_toggle])
                        .output()
                        .is_ok_and(|o| o.status.success());
                    if !ok {
                        let _ = std::process::Command::new("hyprctl")
                            .args(["dispatch", "togglefloating", &win_sel])
                            .output();
                    }
                }

                if float {
                    let lua_resize = format!(
                        r#"hl.dsp.window.resize({{ window = "{win_sel}", x = 460, y = 720 }})"#
                    );
                    let ok_r = std::process::Command::new("hyprctl")
                        .args(["dispatch", &lua_resize])
                        .output()
                        .is_ok_and(|o| o.status.success());
                    if !ok_r {
                        let _ = std::process::Command::new("hyprctl")
                            .args(["dispatch", "resizeactive", "exact", "460", "720", &win_sel])
                            .output();
                    }

                    let lua_center = format!(r#"hl.dsp.window.center({{ window = "{win_sel}" }})"#);
                    let ok_c = std::process::Command::new("hyprctl")
                        .args(["dispatch", &lua_center])
                        .output()
                        .is_ok_and(|o| o.status.success());
                    if !ok_c {
                        let _ = std::process::Command::new("hyprctl")
                            .args(["dispatch", "centerwindow", &win_sel])
                            .output();
                    }
                }
            }
        }
    });
}

/// Read preferences directly from disk without initializing GTK (used by `--waybar` and `--refresh`).
pub fn read_disk() -> Prefs {
    let path = crate::paths::prefs_file();
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default()
}

nixe_core::prefs_store!(Prefs);
