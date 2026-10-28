# Weather — notes for working on this app

- GTK4 (gtk4-rs 0.11) + Rust, no libadwaita, built to `STYLE.md`. The weather logic is
  `crates/nyx-weather-core`; the app is the GTK shell.
- Nyx Design 2.0 (`nyx_theme::use_design_2()`): one blended surface, no lines between
  areas. The unified header (`chrome::app_header`) shows the app, the active location as
  its subtitle and the sidebar toggle; the location search is always in its middle (the
  results drop in as a card under it); at the end, when the forecast was updated (full
  readout in its tooltip), Refresh, Floating and Settings. Narrow windows (floating,
  under 980 px) shorten the search and drop the update time. No status bar: the data's
  credit (`attribution`, linked) ends every forecast page. The sidebar's selection
  (`.active`) is the section; the location in use is a quiet `.current` pill. Chart and
  map labels use the theme's UI font. Settings has the shared Accent row (no Glow).
- Core (`crates/nyx-weather-core`):
  - `model.rs`: the snapshot types and units (temperature, wind, precipitation, pressure,
    distance). `ForecastSnapshot::advance_to(now)` drops past hours and days, expired
    alerts and old nowcast steps, and makes "now" this hour's forecast when the snapshot
    is older; the cache calls it on every read and the window before every render, so
    `daily[0]` is today and `hourly[0]` this hour. `rain_nowcast` reads `minutely_precip`.
  - `open_meteo.rs`: forecast (with `minutely_15`), air quality (hourly lists start at
    the current hour, nulls kept as `None`), geocoding, the model comparison (a second
    request with `models=`; a model that doesn't answer is left out, a missing value is
    NaN), `fetch_field` (one multi-coordinate request), and the customer host when an
    API key is set.
  - `field.rs`: the 7×7 wind/temperature/pressure grid over the map's view, bilinear
    sampling, vector wind, marching-squares contours.
  - `radar.rs`: a map is a `TileView` (the tiles covering the canvas at native size; the
    OSM basemap one zoom in on HiDPI). Radar for US places is IEM's NEXRAD mosaic
    (`iem_radar_frames`: 13 frames 5 minutes apart, times worked out locally, any zoom,
    `/c/` timestamped URLs); elsewhere RainViewer's past frames (10 minutes, 512 px tiles
    one zoom out, zoom ≤ `RAINVIEWER_MAX_ZOOM`, kept under 90 requests a minute by
    `RateLimit`). Satellite is IEM's latest GOES-East IR. Tiles are fetched 12 at a time.
  - `reflectivity.rs`: RainViewer's Universal Blue (rain and snow) and IEM's N0Q colour
    tables, read back to dBZ (every tile pixel is a table colour; others take the
    nearest), and the classic NWS scale they're redrawn in.
  - `cache.rs`: SQLite `weather.db`. Schema changes go through `VERSION` and run once on
    open. Tiles are evicted per kind (basemap / everything else); fields share the tile
    table under `field:` keys.
  - `query.rs`: Glimpse/Assistant questions; a day past the forecast says so.
- App: `backend.rs` keeps one cache connection per thread (`with_cache`); the GTK thread
  only reads. `window.rs`: refreshes carry a sequence number and the location id, so a
  slow one never overwrites a newer one; focusing the window catches up (`catch_up`);
  pages are rebuilt on navigate/refresh and the others dropped after the crossfade, so a
  hidden Radar page's timers stop. Theme changes don't rebuild pages: charts and the map
  `theme::subscribe` and redraw. `notify.rs`: Severe/Extreme alert notifications, ids
  remembered in prefs.
- Radar page (`sections/radar.rs`, `maprender.rs`): workers compose the basemap (device
  resolution) and one picture per frame (canvas resolution) as `Raster`s; the GTK thread
  only paints them. A scene (zoom, layer, size, scale) is composed once; frames load
  newest first, one at a time, and `generation` drops work for an older scene. Playback
  runs on the frame clock (0.7 s per 10 minutes of weather, a 1.5 s rest on the newest),
  cross-fading frame pairs with `Operator::Add` in a group; Reduce motion steps instead.
  Resizes recompose only when the canvas outgrows the pictures.
- Floating mode writes the rule into `~/.config/hypr/nyx.lua` and reloads Hyprland; the
  installer always `require`s that file when Weather is installed.
- `plugin/`: the omarchy-shell bar widget (polls `nyx-weather --waybar`).
- Checks: `cargo test -p nyx-weather-core -p nyx-weather`, clippy `-D warnings` on both.
  Layout check: `WEATHER_SNAPSHOT=/tmp/x.png nyx-weather --section ID` renders the window
  offscreen and quits (`WEATHER_SNAPSHOT_W`/`_H` for the size, `WEATHER_SNAPSHOT_DELAY`
  ms for the forecast to load). Use a different `WEATHER_APP_ID` and scratch
  `XDG_CONFIG_HOME`/`XDG_DATA_HOME`/`XDG_CACHE_HOME`, with `float_window` equal to the
  real setting: the Hyprland rule in `~/.config/hypr/nyx.lua` follows `HOME`, not XDG,
  and changing it reloads Hyprland.
