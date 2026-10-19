//! Provider traits and safe error types for weather and alert services.

use crate::model::{ForecastSnapshot, Location, ProviderAttribution, WeatherAlert};

/// Error messages are safe for display in the UI: never expose raw URLs, API keys,
/// or response payloads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderError {
    Offline,
    RateLimited { retry_after_secs: Option<u64> },
    CommercialKeyRequired,
    InvalidLocation,
    InvalidResponse,
    Failed(String),
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Offline => write!(f, "Weather service is unreachable"),
            Self::RateLimited { retry_after_secs: Some(s) } => {
                write!(f, "Weather service rate limit reached (retry in {s}s)")
            }
            Self::RateLimited { retry_after_secs: None } => {
                write!(f, "Weather service rate limit reached")
            }
            Self::CommercialKeyRequired => write!(
                f,
                "Open-Meteo's free hosted API is for non-commercial use only. Configure an API key or self-hosted endpoint in Settings."
            ),
            Self::InvalidLocation => write!(f, "Location coordinates are invalid"),
            Self::InvalidResponse => write!(f, "Weather service returned an invalid response"),
            Self::Failed(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for ProviderError {}

/// Provider-independent interface for weather forecasts and location geocoding.
pub trait WeatherProvider: Send + Sync {
    /// Stable provider identifier (`"open-meteo"`, `"met-no"`).
    fn id(&self) -> &'static str;

    /// Attribution and licensing details for this provider.
    fn attribution(&self) -> ProviderAttribution;

    /// Search for locations matching `query`.
    fn search_locations(&self, query: &str, limit: usize) -> Result<Vec<Location>, ProviderError>;

    /// Fetch a complete normalized forecast snapshot for `location`.
    fn fetch_forecast(&self, location: &Location) -> Result<ForecastSnapshot, ProviderError>;
}

/// Provider-independent interface for severe weather alerts (e.g., NOAA/NWS).
pub trait AlertProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn fetch_alerts(&self, latitude: f64, longitude: f64) -> Result<Vec<WeatherAlert>, ProviderError>;
}
