# CLAUDE.md (`apps/weather`)

## Architecture

- **`crates/nixe-weather-core`**: Provider-independent weather & radar library.
  - `model.rs`: Normalized domain types (`Location`, `WeatherCondition`, `CurrentConditions`, `HourlyForecast`, `DailyForecast`, `AirQuality`, `WeatherAlert`, `ModelSeries`, `MoonPhaseInfo`, `ForecastSnapshot`), unit conversions (`TemperatureUnit`, `WindSpeedUnit`, `PrecipitationUnit`), Beaufort scale, pollen categories, and astronomical lunar phase calculations.
  - `provider.rs`: `WeatherProvider` trait for pluggable weather backends.
  - `open_meteo.rs`: Open-Meteo Forecast (10-day, 48-hour, solar radiation, visibility, dew point, surface pressure), Air Quality & Pollen (`PM2.5`, `PM10`, `O₃`, `NO₂`, `SO₂`, `CO`, `Dust`, `AOD`, 6 pollen species), Geocoding adapter, and Multi-Model (`Best Match`, `ECMWF IFS`, `NOAA GFS`, `DWD ICON`) comparison series.
  - `radar.rs`: RainViewer global radar/infrared satellite manifest + 3×3 Web Mercator slippy-map tile fetcher (`RadarClient`, `RadarLayerKind`, `TileGridPayload`, `lat_lon_to_tile`).
  - `noaa.rs`: NOAA/NWS active severe-weather alert adapter for US coordinates.
  - `cache.rs`: SQLite forecast, geocoding, and radar/basemap PNG tile cache (`~/.cache/nixe-weather/cache.db`).
  - `query.rs`: Natural-language query parser for Nixe Glimpse (`weather`, `weather tomorrow`, `rain Friday`, `weather London`).
- **`apps/weather` (`nixe-weather`)**:
  - Full Nixe desktop app following `STYLE.md` (`1120×800` default tiled window, `.settings-navigation` collapsible sidebar with `set_hexpand(false)` and `< 980px` auto-compact, `.status-bar` with live readout and actions).
  - Optional **Floating Window** mode (`float_window` in `~/.config/nixe-weather/settings.toml`, toggled in `Settings → Weather window` or via `Ctrl+M`), which updates `~/.config/hypr/nixe.lua` and dispatches `hyprctl` live.
  - `charts.rs`: Theme-reactive Cairo charts (`line_chart`, `solar_arc_canvas`, `moon_phase_canvas`, `draw_radar_map`).
  - `sections/`: `overview.rs`, `hourly.rs`, `daily.rs`, `radar.rs`, `air_quality.rs`, `astronomy.rs`, `settings.rs`.
