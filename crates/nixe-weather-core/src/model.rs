//! Normalized weather domain models and unit conversions.

use serde::{Deserialize, Serialize};

/// Canonical location record used across providers and saved preferences.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Location {
    /// Deterministic coordinate-based identifier (`"{lat:.4},{lon:.4}"`).
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub admin1: Option<String>,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub country_code: Option<String>,
    pub latitude: f64,
    pub longitude: f64,
    #[serde(default = "default_timezone")]
    pub timezone: String,
    #[serde(default)]
    pub elevation_m: Option<f64>,
}

fn default_timezone() -> String {
    "auto".into()
}

impl Location {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        name: impl Into<String>,
        admin1: Option<String>,
        country: Option<String>,
        country_code: Option<String>,
        latitude: f64,
        longitude: f64,
        timezone: Option<String>,
        elevation_m: Option<f64>,
    ) -> Self {
        let id = Self::make_id(latitude, longitude);
        Self {
            id,
            name: name.into(),
            admin1: admin1.filter(|s| !s.trim().is_empty()),
            country: country.filter(|s| !s.trim().is_empty()),
            country_code: country_code.filter(|s| !s.trim().is_empty()),
            latitude,
            longitude,
            timezone: timezone.filter(|s| !s.trim().is_empty()).unwrap_or_else(default_timezone),
            elevation_m,
        }
    }

    pub fn make_id(latitude: f64, longitude: f64) -> String {
        format!("{latitude:.4},{longitude:.4}")
    }

    /// Region and country subtitle, e.g. `"Texas, United States"`.
    pub fn subtitle(&self) -> String {
        match (&self.admin1, &self.country) {
            (Some(a), Some(c)) if a != &self.name && a != c => format!("{a}, {c}"),
            (None, Some(c)) if c != &self.name => c.clone(),
            (Some(a), None) if a != &self.name => a.clone(),
            (Some(_), Some(c)) => c.clone(),
            _ => format!("{:.2}°, {:.2}°", self.latitude, self.longitude),
        }
    }

    /// Full display label (`"Austin, Texas, United States"`).
    pub fn display_name(&self) -> String {
        let sub = self.subtitle();
        if sub.is_empty() {
            self.name.clone()
        } else {
            format!("{} · {}", self.name, sub)
        }
    }

    /// True when the location is within the United States (eligible for NOAA/NWS alerts).
    pub fn is_us(&self) -> bool {
        if let Some(code) = &self.country_code {
            return code.eq_ignore_ascii_case("US");
        }
        if let Some(country) = &self.country {
            let c = country.to_lowercase();
            return c == "united states" || c == "usa" || c == "united states of america";
        }
        false
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TemperatureUnit {
    #[default]
    Celsius,
    Fahrenheit,
}

impl TemperatureUnit {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Celsius => "celsius",
            Self::Fahrenheit => "fahrenheit",
        }
    }

    pub fn from_id(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "fahrenheit" | "f" => Self::Fahrenheit,
            _ => Self::Celsius,
        }
    }

    pub fn symbol(self) -> &'static str {
        match self {
            Self::Celsius => "°C",
            Self::Fahrenheit => "°F",
        }
    }

    pub fn convert(self, celsius: f64) -> f64 {
        match self {
            Self::Celsius => celsius,
            Self::Fahrenheit => celsius * 9.0 / 5.0 + 32.0,
        }
    }

    pub fn format(self, celsius: f64) -> String {
        format!("{:.0}°", self.convert(celsius).round())
    }

    pub fn format_with_unit(self, celsius: f64) -> String {
        format!("{:.0}{}", self.convert(celsius).round(), self.symbol())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WindSpeedUnit {
    #[default]
    Kmh,
    Mph,
    Ms,
    Knots,
}

impl WindSpeedUnit {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Kmh => "kmh",
            Self::Mph => "mph",
            Self::Ms => "ms",
            Self::Knots => "knots",
        }
    }

    pub fn from_id(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "mph" => Self::Mph,
            "ms" | "m/s" => Self::Ms,
            "knots" | "kn" => Self::Knots,
            _ => Self::Kmh,
        }
    }

    pub fn symbol(self) -> &'static str {
        match self {
            Self::Kmh => "km/h",
            Self::Mph => "mph",
            Self::Ms => "m/s",
            Self::Knots => "kn",
        }
    }

    pub fn convert(self, kmh: f64) -> f64 {
        match self {
            Self::Kmh => kmh,
            Self::Mph => kmh * 0.621_371,
            Self::Ms => kmh / 3.6,
            Self::Knots => kmh * 0.539_957,
        }
    }

    pub fn format(self, kmh: f64) -> String {
        match self {
            Self::Ms => format!("{:.1} {}", self.convert(kmh), self.symbol()),
            _ => format!("{:.0} {}", self.convert(kmh).round(), self.symbol()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PrecipitationUnit {
    #[default]
    Millimeters,
    Inches,
}

impl PrecipitationUnit {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Millimeters => "mm",
            Self::Inches => "inch",
        }
    }

    pub fn from_id(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "inch" | "in" | "inches" => Self::Inches,
            _ => Self::Millimeters,
        }
    }

    pub fn symbol(self) -> &'static str {
        match self {
            Self::Millimeters => "mm",
            Self::Inches => "in",
        }
    }

    pub fn convert(self, mm: f64) -> f64 {
        match self {
            Self::Millimeters => mm,
            Self::Inches => mm / 25.4,
        }
    }

    pub fn format(self, mm: f64) -> String {
        match self {
            Self::Millimeters => {
                if mm < 1.0 && mm > 0.0 {
                    format!("{mm:.1} mm")
                } else {
                    format!("{:.0} mm", mm.round())
                }
            }
            Self::Inches => format!("{:.2} in", self.convert(mm)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UnitPrefs {
    pub temperature: TemperatureUnit,
    pub wind_speed: WindSpeedUnit,
    pub precipitation: PrecipitationUnit,
    pub use_24h_time: bool,
}

/// Normalized weather condition across all providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WeatherCondition {
    Clear,
    MostlyClear,
    PartlyCloudy,
    Overcast,
    Fog,
    Drizzle,
    FreezingDrizzle,
    Rain,
    HeavyRain,
    FreezingRain,
    Snow,
    HeavySnow,
    SnowGrains,
    RainShowers,
    SnowShowers,
    Thunderstorm,
    ThunderstormHail,
    #[default]
    Unknown,
}

impl WeatherCondition {
    /// Map WMO weather interpretation code (WW) to [`WeatherCondition`].
    pub fn from_wmo(code: u16) -> Self {
        match code {
            0 => Self::Clear,
            1 => Self::MostlyClear,
            2 => Self::PartlyCloudy,
            3 => Self::Overcast,
            45 | 48 => Self::Fog,
            51 | 53 | 55 => Self::Drizzle,
            56 | 57 => Self::FreezingDrizzle,
            61 | 63 => Self::Rain,
            65 => Self::HeavyRain,
            66 | 67 => Self::FreezingRain,
            71 | 73 => Self::Snow,
            75 => Self::HeavySnow,
            77 => Self::SnowGrains,
            80..=82 => Self::RainShowers,
            85 | 86 => Self::SnowShowers,
            95 => Self::Thunderstorm,
            96 | 99 => Self::ThunderstormHail,
            _ => Self::Unknown,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Clear => "Clear sky",
            Self::MostlyClear => "Mostly clear",
            Self::PartlyCloudy => "Partly cloudy",
            Self::Overcast => "Overcast",
            Self::Fog => "Fog",
            Self::Drizzle => "Light drizzle",
            Self::FreezingDrizzle => "Freezing drizzle",
            Self::Rain => "Rain",
            Self::HeavyRain => "Heavy rain",
            Self::FreezingRain => "Freezing rain",
            Self::Snow => "Snow",
            Self::HeavySnow => "Heavy snow",
            Self::SnowGrains => "Snow grains",
            Self::RainShowers => "Rain showers",
            Self::SnowShowers => "Snow showers",
            Self::Thunderstorm => "Thunderstorm",
            Self::ThunderstormHail => "Thunderstorm with hail",
            Self::Unknown => "Unknown",
        }
    }

    /// Standard freedesktop symbolic icon name for GTK.
    pub fn icon_name(self, is_day: bool) -> &'static str {
        match self {
            Self::Clear | Self::MostlyClear => {
                if is_day {
                    "weather-clear-symbolic"
                } else {
                    "weather-clear-night-symbolic"
                }
            }
            Self::PartlyCloudy => {
                if is_day {
                    "weather-few-clouds-symbolic"
                } else {
                    "weather-few-clouds-night-symbolic"
                }
            }
            Self::Overcast => "weather-overcast-symbolic",
            Self::Fog => "weather-fog-symbolic",
            Self::Drizzle | Self::FreezingDrizzle | Self::RainShowers => "weather-showers-scattered-symbolic",
            Self::Rain | Self::HeavyRain | Self::FreezingRain => "weather-showers-symbolic",
            Self::Snow | Self::HeavySnow | Self::SnowGrains | Self::SnowShowers => "weather-snow-symbolic",
            Self::Thunderstorm | Self::ThunderstormHail => "weather-storm-symbolic",
            Self::Unknown => "weather-overcast-symbolic",
        }
    }

    /// Monospace glyph suitable for Waybar / status bars.
    pub fn waybar_glyph(self, is_day: bool) -> &'static str {
        match self {
            Self::Clear | Self::MostlyClear => {
                if is_day {
                    "☀"
                } else {
                    "☾"
                }
            }
            Self::PartlyCloudy => "⛅",
            Self::Overcast => "☁",
            Self::Fog => "≋",
            Self::Drizzle | Self::FreezingDrizzle | Self::RainShowers => "🌦",
            Self::Rain | Self::HeavyRain | Self::FreezingRain => "🌧",
            Self::Snow | Self::HeavySnow | Self::SnowGrains | Self::SnowShowers => "❄",
            Self::Thunderstorm | Self::ThunderstormHail => "⛈",
            Self::Unknown => "☁",
        }
    }

    /// CSS class identifier for Waybar styling.
    pub fn css_class(self, is_day: bool) -> &'static str {
        match self {
            Self::Clear | Self::MostlyClear => {
                if is_day {
                    "clear-day"
                } else {
                    "clear-night"
                }
            }
            Self::PartlyCloudy => "partly-cloudy",
            Self::Overcast => "overcast",
            Self::Fog => "fog",
            Self::Drizzle | Self::FreezingDrizzle | Self::Rain | Self::HeavyRain | Self::FreezingRain | Self::RainShowers => {
                "rain"
            }
            Self::Snow | Self::HeavySnow | Self::SnowGrains | Self::SnowShowers => "snow",
            Self::Thunderstorm | Self::ThunderstormHail => "storm",
            Self::Unknown => "unknown",
        }
    }

    pub fn is_wet(self) -> bool {
        matches!(
            self,
            Self::Drizzle
                | Self::FreezingDrizzle
                | Self::Rain
                | Self::HeavyRain
                | Self::FreezingRain
                | Self::RainShowers
                | Self::Snow
                | Self::HeavySnow
                | Self::SnowGrains
                | Self::SnowShowers
                | Self::Thunderstorm
                | Self::ThunderstormHail
        )
    }
}

/// Convert compass bearing degrees (0..=360) to 8-point cardinal direction.
pub fn cardinal_direction(degrees: u16) -> &'static str {
    let dirs = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
    let idx = (((degrees as f64 + 22.5).rem_euclid(360.0)) / 45.0).floor() as usize;
    dirs[idx.min(7)]
}

/// Human-readable UV risk level.
pub fn uv_category(uv: f64) -> &'static str {
    match uv {
        x if x < 3.0 => "Low",
        x if x < 6.0 => "Moderate",
        x if x < 8.0 => "High",
        x if x < 11.0 => "Very high",
        _ => "Extreme",
    }
}

/// Human-readable US EPA AQI category.
pub fn us_aqi_category(aqi: u16) -> &'static str {
    match aqi {
        0..=50 => "Good",
        51..=100 => "Moderate",
        101..=150 => "Unhealthy for sensitive groups",
        151..=200 => "Unhealthy",
        201..=300 => "Very unhealthy",
        _ => "Hazardous",
    }
}

/// Beaufort wind scale (0..=12) and descriptive label from wind speed in km/h.
pub fn beaufort_scale(wind_kmh: f64) -> (u8, &'static str) {
    match wind_kmh {
        x if x < 1.0 => (0, "Calm"),
        x if x < 6.0 => (1, "Light air"),
        x if x < 12.0 => (2, "Light breeze"),
        x if x < 20.0 => (3, "Gentle breeze"),
        x if x < 29.0 => (4, "Moderate breeze"),
        x if x < 39.0 => (5, "Fresh breeze"),
        x if x < 50.0 => (6, "Strong breeze"),
        x if x < 62.0 => (7, "High wind"),
        x if x < 75.0 => (8, "Gale"),
        x if x < 89.0 => (9, "Strong gale"),
        x if x < 103.0 => (10, "Storm"),
        x if x < 118.0 => (11, "Violent storm"),
        _ => (12, "Hurricane force"),
    }
}

/// Human-readable pollen category from grains/m³.
pub fn pollen_category(grains: f64) -> &'static str {
    match grains {
        x if x <= 0.5 => "None",
        x if x < 15.0 => "Low",
        x if x < 50.0 => "Moderate",
        x if x < 150.0 => "High",
        _ => "Very high",
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MoonPhaseInfo {
    /// Phase fraction in `0.0..1.0` (`0.0` = New Moon, `0.5` = Full Moon).
    pub phase_fraction: f64,
    /// Illuminated percentage (`0..=100`).
    pub illumination_pct: u8,
    /// Lunar age in days (`0.0..29.53`).
    pub age_days: f64,
    pub phase_name: &'static str,
    pub glyph: &'static str,
    pub days_to_full: f64,
    pub days_to_new: f64,
}

/// Compute lunar phase, illumination, and days to next full/new moon from a Unix epoch timestamp.
pub fn calculate_moon_phase(unix_epoch: i64) -> MoonPhaseInfo {
    // Reference new moon: 2000-01-06 18:14 UTC = 947182440
    const SYNODIC_DAYS: f64 = 29.530588853;
    let days_since_ref = (unix_epoch as f64 - 947_182_440.0) / 86_400.0;
    let age_days = days_since_ref.rem_euclid(SYNODIC_DAYS);
    let phase_fraction = (age_days / SYNODIC_DAYS).clamp(0.0, 1.0);
    let illum = 0.5 * (1.0 - (2.0 * std::f64::consts::PI * phase_fraction).cos());
    let illumination_pct = (illum * 100.0).round().clamp(0.0, 100.0) as u8;

    let (phase_name, glyph) = match phase_fraction {
        f if !(0.03..0.97).contains(&f) => ("New Moon", "🌑"),
        f if f < 0.22 => ("Waxing Crescent", "🌒"),
        f if f < 0.28 => ("First Quarter", "🌓"),
        f if f < 0.47 => ("Waxing Gibbous", "🌔"),
        f if f < 0.53 => ("Full Moon", "🌕"),
        f if f < 0.72 => ("Waning Gibbous", "🌖"),
        f if f < 0.78 => ("Last Quarter", "🌗"),
        _ => ("Waning Crescent", "🌘"),
    };

    let full_age = SYNODIC_DAYS * 0.5;
    let days_to_full = if age_days <= full_age {
        full_age - age_days
    } else {
        SYNODIC_DAYS - age_days + full_age
    };
    let days_to_new = SYNODIC_DAYS - age_days;

    MoonPhaseInfo {
        phase_fraction,
        illumination_pct,
        age_days,
        phase_name,
        glyph,
        days_to_full,
        days_to_new,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CurrentWeather {
    pub time: String,
    pub temperature_c: f64,
    pub apparent_temperature_c: f64,
    pub condition: WeatherCondition,
    pub is_day: bool,
    pub humidity_pct: u8,
    pub dew_point_c: Option<f64>,
    pub wind_speed_kmh: f64,
    pub wind_gusts_kmh: Option<f64>,
    pub wind_direction_deg: u16,
    pub pressure_hpa: Option<f64>,
    pub precipitation_mm: f64,
    pub cloud_cover_pct: u8,
    pub visibility_m: Option<f64>,
    pub uv_index: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HourlyForecast {
    pub time: String,
    pub temperature_c: f64,
    pub apparent_temperature_c: f64,
    pub precipitation_probability_pct: u8,
    pub precipitation_mm: f64,
    pub condition: WeatherCondition,
    pub is_day: bool,
    pub wind_speed_kmh: f64,
    #[serde(default)]
    pub wind_gusts_kmh: Option<f64>,
    #[serde(default)]
    pub wind_direction_deg: Option<u16>,
    pub uv_index: Option<f64>,
    pub humidity_pct: Option<u8>,
    #[serde(default)]
    pub dew_point_c: Option<f64>,
    #[serde(default)]
    pub pressure_hpa: Option<f64>,
    #[serde(default)]
    pub cloud_cover_pct: Option<u8>,
    #[serde(default)]
    pub visibility_m: Option<f64>,
    #[serde(default)]
    pub shortwave_radiation_wm2: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DailyForecast {
    pub date: String,
    pub condition: WeatherCondition,
    pub temp_max_c: f64,
    pub temp_min_c: f64,
    pub apparent_max_c: Option<f64>,
    pub apparent_min_c: Option<f64>,
    pub sunrise: Option<String>,
    pub sunset: Option<String>,
    pub precipitation_sum_mm: f64,
    pub precipitation_probability_max_pct: u8,
    #[serde(default)]
    pub precipitation_hours: Option<f64>,
    pub uv_index_max: Option<f64>,
    pub wind_speed_max_kmh: f64,
    #[serde(default)]
    pub wind_gusts_max_kmh: Option<f64>,
    pub wind_direction_dominant_deg: Option<u16>,
    #[serde(default)]
    pub daylight_duration_secs: Option<f64>,
    #[serde(default)]
    pub sunshine_duration_secs: Option<f64>,
    #[serde(default)]
    pub shortwave_radiation_sum_mj: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AirQuality {
    pub us_aqi: Option<u16>,
    pub european_aqi: Option<u16>,
    pub pm2_5: Option<f64>,
    pub pm10: Option<f64>,
    pub ozone: Option<f64>,
    pub nitrogen_dioxide: Option<f64>,
    #[serde(default)]
    pub sulphur_dioxide: Option<f64>,
    #[serde(default)]
    pub carbon_monoxide: Option<f64>,
    #[serde(default)]
    pub aerosol_optical_depth: Option<f64>,
    #[serde(default)]
    pub dust: Option<f64>,
    #[serde(default)]
    pub uv_index_clear_sky: Option<f64>,
    #[serde(default)]
    pub alder_pollen: Option<f64>,
    #[serde(default)]
    pub birch_pollen: Option<f64>,
    #[serde(default)]
    pub grass_pollen: Option<f64>,
    #[serde(default)]
    pub mugwort_pollen: Option<f64>,
    #[serde(default)]
    pub olive_pollen: Option<f64>,
    #[serde(default)]
    pub ragweed_pollen: Option<f64>,
    #[serde(default)]
    pub hourly_us_aqi: Vec<u16>,
    #[serde(default)]
    pub hourly_pm2_5: Vec<f64>,
    #[serde(default)]
    pub hourly_uv: Vec<f64>,
}

/// Multi-model comparison series (e.g. ECMWF IFS, NOAA GFS, DWD ICON).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelSeries {
    pub model_id: String,
    pub model_name: String,
    pub resolution: String,
    pub daily_max_c: Vec<f64>,
    pub daily_min_c: Vec<f64>,
    pub daily_precip_mm: Vec<f64>,
    pub hourly_temp_c: Vec<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AlertSeverity {
    #[default]
    Minor,
    Moderate,
    Severe,
    Extreme,
}

impl AlertSeverity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Minor => "Advisory",
            Self::Moderate => "Watch",
            Self::Severe => "Severe",
            Self::Extreme => "Extreme",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeatherAlert {
    pub id: String,
    pub event: String,
    pub headline: String,
    pub description: String,
    pub instruction: Option<String>,
    pub severity: AlertSeverity,
    pub area: String,
    pub effective: Option<String>,
    pub expires: Option<String>,
    pub sender: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderAttribution {
    pub provider_id: String,
    pub provider_name: String,
    pub license_name: String,
    pub attribution_text: String,
    pub attribution_url: String,
    pub commercial_use_allowed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForecastSnapshot {
    pub location: Location,
    pub fetched_at: i64,
    pub utc_offset_seconds: i32,
    pub current: CurrentWeather,
    pub hourly: Vec<HourlyForecast>,
    pub daily: Vec<DailyForecast>,
    pub air_quality: Option<AirQuality>,
    #[serde(default)]
    pub alerts: Vec<WeatherAlert>,
    #[serde(default)]
    pub models: Vec<ModelSeries>,
    pub attribution: ProviderAttribution,
}

impl ForecastSnapshot {
    /// Today's high and low in Celsius, falling back to current temperature.
    pub fn today_high_low_c(&self) -> (f64, f64) {
        self.daily
            .first()
            .map(|d| (d.temp_max_c, d.temp_min_c))
            .unwrap_or((self.current.temperature_c, self.current.temperature_c))
    }

    /// Up to `count` hourly entries starting at or after the current observation time.
    pub fn upcoming_hours(&self, count: usize) -> Vec<&HourlyForecast> {
        let current_prefix = self.current.time.get(..13).unwrap_or(&self.current.time);
        let start = self
            .hourly
            .iter()
            .position(|h| h.time.as_str() >= current_prefix)
            .unwrap_or(0);
        self.hourly.iter().skip(start).take(count).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_units_accurately() {
        assert_eq!(TemperatureUnit::Celsius.format(20.0), "20°");
        assert_eq!(TemperatureUnit::Fahrenheit.format(0.0), "32°");
        assert_eq!(TemperatureUnit::Fahrenheit.format(20.0), "68°");
        assert_eq!(WindSpeedUnit::Kmh.format(18.0), "18 km/h");
        assert_eq!(WindSpeedUnit::Ms.format(18.0), "5.0 m/s");
        assert_eq!(WindSpeedUnit::Mph.format(16.0934), "10 mph");
        assert_eq!(PrecipitationUnit::Millimeters.format(12.4), "12 mm");
        assert_eq!(PrecipitationUnit::Inches.format(25.4), "1.00 in");
    }

    #[test]
    fn maps_wmo_codes_and_cardinals() {
        assert_eq!(WeatherCondition::from_wmo(0), WeatherCondition::Clear);
        assert_eq!(WeatherCondition::from_wmo(2), WeatherCondition::PartlyCloudy);
        assert_eq!(WeatherCondition::from_wmo(63), WeatherCondition::Rain);
        assert_eq!(WeatherCondition::from_wmo(95), WeatherCondition::Thunderstorm);
        assert_eq!(cardinal_direction(0), "N");
        assert_eq!(cardinal_direction(45), "NE");
        assert_eq!(cardinal_direction(180), "S");
        assert_eq!(cardinal_direction(315), "NW");
        assert_eq!(cardinal_direction(359), "N");
        let moon = calculate_moon_phase(1_729_000_000);
        assert!(moon.illumination_pct <= 100);
        assert!(!moon.phase_name.is_empty());
    }
}
