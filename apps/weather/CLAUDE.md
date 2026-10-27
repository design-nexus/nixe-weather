# Weather — notes for working on this app

- GTK4 (gtk4-rs 0.11) + Rust, no libadwaita, built to `STYLE.md`. The weather logic is
  `crates/nyx-weather-core`; the app is the GTK shell.
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
  - `field.rs`: the 7×7 wind/temperature/pressure grid over the 3×3 tile map, bilinear
    sampling, vector wind, marching-squares contours.
  - `radar.rs`: RainViewer manifest and tiles (zoom ≤ `RAINVIEWER_MAX_ZOOM`), IEM tiles
    above that and as the satellite fallback (5-minute TTL; RainViewer URLs carry their
    time), OSM basemap.
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
- Floating mode writes the rule into `~/.config/hypr/nyx.lua` and reloads Hyprland; the
  installer always `require`s that file when Weather is installed.
- `plugin/`: the omarchy-shell bar widget (polls `nyx-weather --waybar`).
- Checks: `cargo test -p nyx-weather-core -p nyx-weather`, clippy `-D warnings` on both.
