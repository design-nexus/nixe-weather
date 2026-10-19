//! Worker-thread weather fetching, radar tile loading, caching, and offline fallback orchestration.

use crate::paths;
use nixe_weather_core::{
    Cache, ForecastSnapshot, Location, OpenMeteoConfig, OpenMeteoProvider, ProviderError,
    RadarClient, RadarFrame, RadarLayerKind, RadarManifest, TileGridPayload, WeatherProvider,
};
use std::time::{SystemTime, UNIX_EPOCH};

const GEOCODING_TTL_SECS: i64 = 24 * 3600;

#[derive(Debug, Clone)]
pub struct FetchOutcome {
    pub snapshot: Option<ForecastSnapshot>,
    pub stale_fallback: bool,
    pub error: Option<ProviderError>,
}

pub fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Format how long ago `fetched_at` occurred (e.g. `"just now"`, `"4m ago"`, `"2h ago"`).
pub fn human_age(fetched_at: i64) -> String {
    let diff = now_epoch().saturating_sub(fetched_at);
    if diff < 60 {
        "just now".into()
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else {
        format!("{}d ago", diff / 86400)
    }
}

/// Read cached snapshot immediately without touching the network.
pub fn get_cached(location_id: &str) -> Option<ForecastSnapshot> {
    let cache = Cache::open(&paths::cache_db()).ok()?;
    cache.get_forecast(location_id).ok().flatten()
}

/// Load a fresh cached forecast or fetch from Open-Meteo on a worker thread,
/// preserving and returning any cached snapshot if the network is unreachable.
pub fn load_or_fetch(
    location: &Location,
    config: &OpenMeteoConfig,
    max_age_secs: i64,
    force_refresh: bool,
) -> FetchOutcome {
    let now = now_epoch();
    let db_path = paths::cache_db();
    let mut cache = Cache::open(&db_path).ok();

    let cached_snapshot = cache
        .as_ref()
        .and_then(|c| c.get_forecast(&location.id).ok().flatten());
    let is_stale = cache
        .as_ref()
        .and_then(|c| c.is_stale(&location.id, max_age_secs, now).ok())
        .unwrap_or(true);

    if !force_refresh
        && !is_stale
        && let Some(snap) = cached_snapshot
    {
        return FetchOutcome {
            snapshot: Some(snap),
            stale_fallback: false,
            error: None,
        };
    }

    let provider = OpenMeteoProvider::new(config.clone());
    match provider.fetch_forecast(location) {
        Ok(fresh) => {
            if let Some(c) = cache.as_mut() {
                let _ = c.put_forecast(&fresh);
            }
            FetchOutcome {
                snapshot: Some(fresh),
                stale_fallback: false,
                error: None,
            }
        }
        Err(err) => FetchOutcome {
            stale_fallback: cached_snapshot.is_some(),
            snapshot: cached_snapshot,
            error: Some(err),
        },
    }
}

/// Search locations using the SQLite geocoding cache first, then Open-Meteo Geocoding API.
pub fn search_locations(
    query: &str,
    config: &OpenMeteoConfig,
) -> Result<Vec<Location>, ProviderError> {
    let q = query.trim();
    if q.len() < 2 {
        return Ok(Vec::new());
    }
    let now = now_epoch();
    let db_path = paths::cache_db();
    let mut cache = Cache::open(&db_path).ok();

    if let Some(cached) = cache
        .as_ref()
        .and_then(|c| c.get_geocoding(q, GEOCODING_TTL_SECS, now).ok().flatten())
        && !cached.is_empty()
    {
        return Ok(cached);
    }

    let provider = OpenMeteoProvider::new(config.clone());
    let results = provider.search_locations(q, 8)?;
    if let Some(c) = cache.as_mut() {
        let _ = c.put_geocoding(q, &results, now);
    }
    Ok(results)
}

fn radar_client() -> &'static RadarClient {
    static CLIENT: std::sync::OnceLock<RadarClient> = std::sync::OnceLock::new();
    CLIENT.get_or_init(RadarClient::new)
}

fn manifest_cache() -> &'static std::sync::Mutex<Option<RadarManifest>> {
    static MANIFEST: std::sync::OnceLock<std::sync::Mutex<Option<RadarManifest>>> =
        std::sync::OnceLock::new();
    MANIFEST.get_or_init(|| std::sync::Mutex::new(None))
}

/// Return cached RainViewer manifest if fresh (< 5 minutes old), or fetch and cache it.
pub fn fetch_radar_manifest() -> Result<RadarManifest, ProviderError> {
    let now = now_epoch();
    if let Ok(guard) = manifest_cache().lock()
        && let Some(m) = guard.as_ref()
        && now.saturating_sub(m.fetched_at) < 300
    {
        return Ok(m.clone());
    }
    let m = radar_client().fetch_manifest()?;
    if let Ok(mut guard) = manifest_cache().lock() {
        *guard = Some(m.clone());
    }
    Ok(m)
}

/// Synchronously read any cached basemap + overlay tiles from SQLite without network I/O.
pub fn get_cached_radar_grid(
    lat: f64,
    lon: f64,
    z: u8,
    host: &str,
    frame: Option<&RadarFrame>,
    layer: RadarLayerKind,
) -> Option<TileGridPayload> {
    let cache = Cache::open(&paths::cache_db()).ok()?;
    RadarClient::get_cached_grid(lat, lon, z, host, frame, layer, &cache)
}

/// Fetch only the 3×3 background basemap tiles (cached in SQLite, parallel HTTP on cache miss).
pub fn fetch_basemap_grid(lat: f64, lon: f64, z: u8) -> TileGridPayload {
    let mut cache = Cache::open(&paths::cache_db()).ok();
    radar_client().fetch_basemap_grid(lat, lon, z, cache.as_mut())
}

/// Fetch only the 3×3 radar/satellite overlay tiles in parallel.
pub fn fetch_overlay_tiles(
    lat: f64,
    lon: f64,
    z: u8,
    host: &str,
    frame: Option<&RadarFrame>,
    layer: RadarLayerKind,
) -> Vec<Option<Vec<u8>>> {
    let mut cache = Cache::open(&paths::cache_db()).ok();
    radar_client().fetch_overlay_tiles(lat, lon, z, host, frame, layer, cache.as_mut())
}

/// Pre-warm the Z6 basemap and latest radar frame in a background thread so opening Radar is instant.
pub fn prewarm_radar(lat: f64, lon: f64) {
    std::thread::spawn(move || {
        let _ = fetch_basemap_grid(lat, lon, 6);
        if let Ok(m) = fetch_radar_manifest()
            && let Some(last) = m.radar_frames.last()
        {
            let _ = fetch_overlay_tiles(
                lat,
                lon,
                6,
                &m.host,
                Some(last),
                RadarLayerKind::PrecipitationRadar,
            );
        }
    });
}
