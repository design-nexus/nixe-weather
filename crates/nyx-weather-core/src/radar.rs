//! Weather radar and infrared satellite tile client (RainViewer, with Iowa Environmental
//! Mesonet tiles for close-in US radar), over an OpenStreetMap basemap.

use crate::cache::Cache;
use crate::provider::ProviderError;
use reqwest::blocking::Client;
use reqwest::redirect;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Read;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const RAINVIEWER_MAPS_URL: &str = "https://api.rainviewer.com/public/weather-maps.json";
const MAX_TILE_BYTES: usize = 512 * 1024;
const BASEMAP_TTL_SECS: i64 = 7 * 86_400;
/// RainViewer tile URLs name their frame's time, so a cached one never goes stale.
const RADAR_TILE_TTL_SECS: i64 = 3 * 3600;
/// IEM tiles are always "latest" under the same URL.
const LIVE_TILE_TTL_SECS: i64 = 5 * 60;

/// How long a cached overlay tile at `url` stays current.
fn overlay_ttl(url: &str) -> i64 {
    if url.contains("mesonet.agron.iastate.edu") { LIVE_TILE_TTL_SECS } else { RADAR_TILE_TTL_SECS }
}

/// The highest zoom RainViewer serves; closer in, radar is the US-only NEXRAD mosaic.
pub const RAINVIEWER_MAX_ZOOM: u8 = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RadarLayerKind {
    #[default]
    PrecipitationRadar,
    InfraredSatellite,
    WindStreamlines,
    TemperatureContour,
}

impl RadarLayerKind {
    pub fn id(self) -> &'static str {
        match self {
            Self::PrecipitationRadar => "radar",
            Self::InfraredSatellite => "satellite",
            Self::WindStreamlines => "wind",
            Self::TemperatureContour => "temp",
        }
    }

    pub fn from_id(id: &str) -> Self {
        match id {
            "satellite" => Self::InfraredSatellite,
            "wind" => Self::WindStreamlines,
            "temp" => Self::TemperatureContour,
            _ => Self::PrecipitationRadar,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::PrecipitationRadar => "Precipitation Radar",
            Self::InfraredSatellite => "Infrared Satellite",
            Self::WindStreamlines => "Wind",
            Self::TemperatureContour => "Temp & Pressure",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::PrecipitationRadar => "RainViewer radar composite, past two hours (dBZ).",
            Self::InfraredSatellite => "Infrared cloud-top imagery.",
            Self::WindStreamlines => "Current 10 m wind over the map, from a 7×7 grid of Open-Meteo points.",
            Self::TemperatureContour => "Current 2 m temperature and sea-level isobars (every 2 hPa), from a 7×7 grid of Open-Meteo points.",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RadarFrame {
    pub time: i64,
    pub path: String,
    pub is_nowcast: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RadarManifest {
    pub host: String,
    pub radar_frames: Vec<RadarFrame>,
    pub satellite_frames: Vec<RadarFrame>,
    pub fetched_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TileCoord {
    pub z: u8,
    pub x: i32,
    pub y: i32,
    /// Fractional offset `(0.0..1.0)` inside the center tile `(x, y)`.
    pub frac_x: f64,
    pub frac_y: f64,
}

/// Convert WGS84 `(lat, lon)` and zoom `z` (`1..=12`) into Web Mercator tile coordinates.
pub fn lat_lon_to_tile(lat: f64, lon: f64, z: u8) -> TileCoord {
    let z_clamped = z.clamp(1, 12);
    let n = (1u32 << z_clamped) as f64;
    let lat_rad = lat.clamp(-85.0511, 85.0511).to_radians();
    let xf = (lon + 180.0) / 360.0 * n;
    let yf = (1.0 - (lat_rad.tan() + 1.0 / lat_rad.cos()).ln() / std::f64::consts::PI) / 2.0 * n;
    let x = xf.floor() as i32;
    let y = yf.floor() as i32;
    TileCoord {
        z: z_clamped,
        x,
        y,
        frac_x: xf.fract(),
        frac_y: yf.fract(),
    }
}

/// Grid of 3×3 PNG tiles centered on `center` for compositing a 768×768 map canvas.
#[derive(Debug, Clone)]
pub struct TileGridPayload {
    pub center: TileCoord,
    /// 9 basemap PNG byte buffers in row-major order `(dy: -1..=1, dx: -1..=1)`.
    pub basemap_tiles: Vec<Option<Vec<u8>>>,
    /// 9 radar/satellite overlay PNG byte buffers in row-major order `(dy: -1..=1, dx: -1..=1)`.
    pub overlay_tiles: Vec<Option<Vec<u8>>>,
    pub frame_time: Option<i64>,
    pub is_nowcast: bool,
}

pub struct RadarClient {
    http: Client,
}

impl Default for RadarClient {
    fn default() -> Self {
        Self::new()
    }
}

impl RadarClient {
    pub fn new() -> Self {
        let http = Client::builder()
            .user_agent("nyx-weather/0.1 (https://github.com/design-nexus/nyx)")
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(12))
            .redirect(redirect::Policy::none())
            .build()
            .unwrap_or_else(|_| Client::new());
        Self { http }
    }

    pub fn fetch_manifest(&self) -> Result<RadarManifest, ProviderError> {
        let resp = self
            .http
            .get(RAINVIEWER_MAPS_URL)
            .send()
            .map_err(|e| {
                if e.is_connect() || e.is_timeout() {
                    ProviderError::Offline
                } else {
                    ProviderError::Failed("Could not reach RainViewer radar service.".into())
                }
            })?;
        if !resp.status().is_success() {
            return Err(ProviderError::Failed(format!(
                "RainViewer returned HTTP {}",
                resp.status().as_u16()
            )));
        }
        let mut body = Vec::new();
        resp.take(512 * 1024)
            .read_to_end(&mut body)
            .map_err(|_| ProviderError::InvalidResponse)?;
        let val: Value = serde_json::from_slice(&body).map_err(|_| ProviderError::InvalidResponse)?;
        Self::parse_manifest_json(&val)
    }

    pub fn parse_manifest_json(val: &Value) -> Result<RadarManifest, ProviderError> {
        let host = val["host"]
            .as_str()
            .unwrap_or("https://tilecache.rainviewer.com")
            .trim_end_matches('/')
            .to_string();

        let mut radar_frames = Vec::new();
        if let Some(past) = val["radar"]["past"].as_array() {
            for item in past {
                if let (Some(time), Some(path)) = (item["time"].as_i64(), item["path"].as_str()) {
                    radar_frames.push(RadarFrame {
                        time,
                        path: path.to_string(),
                        is_nowcast: false,
                    });
                }
            }
        }
        if let Some(nowcast) = val["radar"]["nowcast"].as_array() {
            for item in nowcast {
                if let (Some(time), Some(path)) = (item["time"].as_i64(), item["path"].as_str()) {
                    radar_frames.push(RadarFrame {
                        time,
                        path: path.to_string(),
                        is_nowcast: true,
                    });
                }
            }
        }

        let mut satellite_frames = Vec::new();
        if let Some(infrared) = val["satellite"]["infrared"].as_array() {
            for item in infrared {
                if let (Some(time), Some(path)) = (item["time"].as_i64(), item["path"].as_str()) {
                    satellite_frames.push(RadarFrame {
                        time,
                        path: path.to_string(),
                        is_nowcast: false,
                    });
                }
            }
        }

        let fetched_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        if satellite_frames.is_empty() {
            satellite_frames.push(RadarFrame {
                time: fetched_at,
                path: "iem:goes-east-ir-4km-900913".to_string(),
                is_nowcast: false,
            });
        }

        Ok(RadarManifest {
            host,
            radar_frames,
            satellite_frames,
            fetched_at,
        })
    }

    fn is_invalid_rainviewer_tile(url: &str, bytes: &[u8]) -> bool {
        // RainViewer answers an unavailable tile with one fixed placeholder image.
        url.contains("rainviewer.com") && bytes.len() == 1370
    }

    fn ttl(url: &str) -> i64 {
        if url.contains("tile.openstreetmap.org") { BASEMAP_TTL_SECS } else { overlay_ttl(url) }
    }

    fn fetch_bytes_http(&self, url: &str) -> Option<Vec<u8>> {
        let resp = self.http.get(url).send().ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let mut buf = Vec::new();
        resp.take((MAX_TILE_BYTES + 1) as u64).read_to_end(&mut buf).ok()?;
        if buf.is_empty() || buf.len() > MAX_TILE_BYTES || Self::is_invalid_rainviewer_tile(url, &buf)
        {
            return None;
        }
        Some(buf)
    }

    /// Fetch a batch of `(index, url)` tiles: reads cached tiles from SQLite first,
    /// then fetches any cache misses in parallel over the shared HTTP connection pool,
    /// and writes newly fetched tiles back to SQLite.
    fn fetch_tile_batch(
        &self,
        urls: Vec<Option<String>>,
        now: i64,
        mut cache: Option<&mut Cache>,
    ) -> Vec<Option<Vec<u8>>> {
        let mut results: Vec<Option<Vec<u8>>> = vec![None; urls.len()];
        let mut misses: Vec<(usize, String)> = Vec::new();

        for (idx, url_opt) in urls.into_iter().enumerate() {
            let Some(url) = url_opt else { continue };
            if let Some(c) = cache.as_deref()
                && let Ok(Some(bytes)) = c.get_tile(&url, Self::ttl(&url), now)
                && !Self::is_invalid_rainviewer_tile(&url, &bytes)
            {
                results[idx] = Some(bytes);
            } else {
                misses.push((idx, url));
            }
        }

        if !misses.is_empty() {
            let fetched: Vec<(usize, String, Option<Vec<u8>>)> = std::thread::scope(|s| {
                let handles: Vec<_> = misses
                    .into_iter()
                    .map(|(idx, url)| {
                        s.spawn(move || {
                            let bytes = self.fetch_bytes_http(&url);
                            (idx, url, bytes)
                        })
                    })
                    .collect();
                handles.into_iter().filter_map(|h| h.join().ok()).collect()
            });

            for (idx, url, bytes_opt) in fetched {
                if let Some(bytes) = bytes_opt {
                    if let Some(c) = cache.as_deref_mut() {
                        let _ = c.put_tile(&url, &bytes, now);
                    }
                    results[idx] = Some(bytes);
                }
            }
        }

        results
    }

    fn grid_urls(
        center: TileCoord,
        host: &str,
        frame: Option<&RadarFrame>,
        layer: RadarLayerKind,
    ) -> (Vec<Option<String>>, Vec<Option<String>>) {
        let n = 1i32 << center.z;
        let mut base_urls = Vec::with_capacity(9);
        let mut ov_urls = Vec::with_capacity(9);

        for dy in -1..=1 {
            for dx in -1..=1 {
                let tx = (center.x + dx).rem_euclid(n);
                let ty = center.y + dy;
                if ty < 0 || ty >= n {
                    base_urls.push(None);
                    ov_urls.push(None);
                    continue;
                }
                base_urls.push(Some(format!(
                    "https://tile.openstreetmap.org/{}/{tx}/{ty}.png",
                    center.z
                )));

                let ov_url = match (layer, frame) {
                    // RainViewer public tiles support z <= 7; for z > 7 use high-res IEM NEXRAD.
                    (RadarLayerKind::PrecipitationRadar, Some(f)) if center.z <= RAINVIEWER_MAX_ZOOM => Some(format!(
                        "{}{}/256/{}/{tx}/{ty}/2/1_1.png",
                        host.trim_end_matches('/'),
                        f.path,
                        center.z
                    )),
                    (RadarLayerKind::PrecipitationRadar, _) => Some(format!(
                        "https://mesonet.agron.iastate.edu/cache/tile.py/1.0.0/nexrad-n0q-900913/{}/{tx}/{ty}.png",
                        center.z
                    )),
                    (RadarLayerKind::InfraredSatellite, Some(f))
                        if !f.path.starts_with("iem:") =>
                    {
                        Some(format!(
                            "{}{}/256/{}/{tx}/{ty}/0/0_0.png",
                            host.trim_end_matches('/'),
                            f.path,
                            center.z
                        ))
                    }
                    (RadarLayerKind::InfraredSatellite, _) => Some(format!(
                        "https://mesonet.agron.iastate.edu/cache/tile.py/1.0.0/goes-east-ir-4km-900913/{}/{tx}/{ty}.png",
                        center.z
                    )),
                    _ => None,
                };
                ov_urls.push(ov_url);
            }
        }
        (base_urls, ov_urls)
    }

    /// Read whatever basemap and overlay tiles are already in SQLite without any network I/O.
    pub fn get_cached_grid(
        lat: f64,
        lon: f64,
        z: u8,
        host: &str,
        frame: Option<&RadarFrame>,
        layer: RadarLayerKind,
        cache: &Cache,
    ) -> Option<TileGridPayload> {
        let center = lat_lon_to_tile(lat, lon, z);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let (base_urls, ov_urls) = Self::grid_urls(center, host, frame, layer);

        let mut basemap_tiles = Vec::with_capacity(9);
        let mut any_base = false;
        for u in base_urls {
            let tile = u.and_then(|url| {
                cache
                    .get_tile(&url, BASEMAP_TTL_SECS, now)
                    .ok()
                    .flatten()
                    .filter(|b| !Self::is_invalid_rainviewer_tile(&url, b))
            });
            if tile.is_some() {
                any_base = true;
            }
            basemap_tiles.push(tile);
        }
        if !any_base {
            return None;
        }

        let mut overlay_tiles = Vec::with_capacity(9);
        for u in ov_urls {
            let tile = u.and_then(|url| {
                cache
                    .get_tile(&url, overlay_ttl(&url), now)
                    .ok()
                    .flatten()
                    .filter(|b| !Self::is_invalid_rainviewer_tile(&url, b))
            });
            overlay_tiles.push(tile);
        }

        Some(TileGridPayload {
            center,
            basemap_tiles,
            overlay_tiles,
            frame_time: frame.map(|f| f.time),
            is_nowcast: frame.is_some_and(|f| f.is_nowcast),
        })
    }

    /// Fetch only the 3×3 background basemap tiles in parallel (using SQLite cache first).
    pub fn fetch_basemap_grid(
        &self,
        lat: f64,
        lon: f64,
        z: u8,
        cache: Option<&mut Cache>,
    ) -> TileGridPayload {
        let center = lat_lon_to_tile(lat, lon, z);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let (base_urls, _) =
            Self::grid_urls(center, "", None, RadarLayerKind::WindStreamlines);
        let basemap_tiles = self.fetch_tile_batch(base_urls, now, cache);
        TileGridPayload {
            center,
            basemap_tiles,
            overlay_tiles: vec![None; 9],
            frame_time: None,
            is_nowcast: false,
        }
    }

    /// Fetch only the 3×3 radar/satellite overlay tiles in parallel (using SQLite cache first).
    #[allow(clippy::too_many_arguments)]
    pub fn fetch_overlay_tiles(
        &self,
        lat: f64,
        lon: f64,
        z: u8,
        host: &str,
        frame: Option<&RadarFrame>,
        layer: RadarLayerKind,
        cache: Option<&mut Cache>,
    ) -> Vec<Option<Vec<u8>>> {
        let center = lat_lon_to_tile(lat, lon, z);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let (_, ov_urls) = Self::grid_urls(center, host, frame, layer);
        self.fetch_tile_batch(ov_urls, now, cache)
    }

    /// Fetch a 3×3 grid of basemap + radar/satellite overlay PNG tiles around `(lat, lon)` at zoom `z`.
    #[allow(clippy::too_many_arguments)]
    pub fn fetch_grid(
        &self,
        lat: f64,
        lon: f64,
        z: u8,
        _light_basemap: bool,
        host: &str,
        frame: Option<&RadarFrame>,
        layer: RadarLayerKind,
        mut cache: Option<&mut Cache>,
    ) -> TileGridPayload {
        let center = lat_lon_to_tile(lat, lon, z);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let (base_urls, ov_urls) = Self::grid_urls(center, host, frame, layer);
        let basemap_tiles = self.fetch_tile_batch(base_urls, now, cache.as_deref_mut());
        let overlay_tiles = self.fetch_tile_batch(ov_urls, now, cache);

        TileGridPayload {
            center,
            basemap_tiles,
            overlay_tiles,
            frame_time: frame.map(|f| f.time),
            is_nowcast: frame.is_some_and(|f| f.is_nowcast),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_slippy_tile_coordinates_and_parses_manifest() {
        let tc = lat_lon_to_tile(40.7128, -74.0060, 6);
        assert_eq!(tc.z, 6);
        assert_eq!(tc.x, 18);
        assert_eq!(tc.y, 24);
        assert!((0.0..1.0).contains(&tc.frac_x));
        assert!((0.0..1.0).contains(&tc.frac_y));

        let sample = serde_json::json!({
            "host": "https://tilecache.rainviewer.com",
            "radar": {
                "past": [{"time": 1729280000, "path": "/v2/radar/1729280000"}],
                "nowcast": [{"time": 1729280600, "path": "/v2/radar/1729280600"}]
            },
            "satellite": {
                "infrared": [{"time": 1729280000, "path": "/v2/satellite/1729280000"}]
            }
        });
        let manifest = RadarClient::parse_manifest_json(&sample).unwrap();
        assert_eq!(manifest.radar_frames.len(), 2);
        assert!(!manifest.radar_frames[0].is_nowcast);
        assert!(manifest.radar_frames[1].is_nowcast);
        assert_eq!(manifest.satellite_frames.len(), 1);
    }
}
