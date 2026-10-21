//! Provider-independent weather models, unit conversions, provider adapters,
//! natural query evaluation, and SQLite caching for Weather.

pub mod cache;
pub mod field;
pub mod model;
pub mod noaa;
pub mod open_meteo;
pub mod provider;
pub mod query;
pub mod radar;

pub use cache::Cache;
pub use field::{Bounds, FIELD_SIZE, FieldGrid, FieldPoint};
pub use model::*;
pub use noaa::NoaaAlertProvider;
pub use open_meteo::{OpenMeteoConfig, OpenMeteoProvider};
pub use provider::{AlertProvider, ProviderError, WeatherProvider};
pub use query::{QueryIntent, QueryResponse, evaluate_query, parse_query};
pub use radar::{
    RAINVIEWER_MAX_ZOOM, RadarClient, RadarFrame, RadarLayerKind, RadarManifest, TileCoord, TileGridPayload,
    lat_lon_to_tile,
};
