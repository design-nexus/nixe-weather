# Weather (`nixe-weather`)

A full-fledged native Linux desktop weather application for the **Nixe** suite, built with Rust, GTK 4, and Cairo, optimized for Omarchy and Hyprland.

## Features

- **Full Nixe App Layout & Optional Floating Mode:**
  - Opens as a standard tiled Nixe application (`1120×800`) with a collapsible `.settings-navigation` sidebar (`Ctrl+B`, or automatically icon-only below `980px`) and saved locations in the sidebar.
  - **Floating Window Option (`Settings → Weather window` or `Ctrl+M`):** Toggle between standard Hyprland tiling and a floating centered window (`1040×740`) at any time. Writes to `~/.config/hypr/nixe.lua` and reloads/toggles Hyprland live via `hyprctl`.
- **6 Dedicated Meteorological Sections (`Alt+1`–`Alt+6`):**
  1. **Overview (`overview`):**
     - Current location, condition icon, temperature, feels-like, and high/low summary.
     - Active **NOAA / NWS** severe-weather alert banner (expandable, hidden when none are active).
     - Horizontally scrollable **24-hour forecast** strip.
     - **7-day forecast** with proportional temperature range bars and precipitation probabilities.
     - Expandable condition cards for **Wind & Gusts (with Beaufort scale)**, **Humidity / Dew Point / Surface Pressure / Visibility / Cloud Cover**, **UV Index**, **Air Quality (US EPA AQI & PM2.5/PM10)**, and **Sunrise / Sunset / Daylight / Solar Radiation**.
  2. **Hourly & Graphs (`hourly`):**
     - **48-Hour Temperature, Feels-Like & Precipitation Probability** Cairo chart.
     - **48-Hour Wind Speed & Gusts** Cairo chart.
     - **48-Hour Relative Humidity & Cloud Cover** Cairo chart.
     - Complete 48-hour meteorological breakdown table.
  3. **10-Day Outlook (`daily`):**
     - **10-Day High / Low & Precipitation Trend** Cairo chart.
     - Expandable **10-day forecast cards** with apparent high/low, precipitation sum/hours/probability, max wind & gusts, dominant wind direction, UV max, sunrise/sunset, daylight/sunshine duration, and shortwave solar radiation (`MJ/m²`).
  4. **Radar & Overlays (`radar`):**
     - Interactive multi-layer map studio over a 3×3 Web Mercator CartoDB dark/light slippy-map tile grid cached in SQLite:
       - **Precipitation Radar:** Live RainViewer global Doppler radar reflectivity (`dBZ`) + 30-minute radar nowcast frames.
       - **Infrared Satellite:** RainViewer global infrared cloud-top coverage tiles.
       - **Wind & Streamlines:** Animated surface wind streamlines, directional vector arrows, and isotach rings.
       - **Temp & Isobars:** Regional 2m temperature gradient field with sea-level pressure (`hPa`) isobar contours.
     - Includes **Zoom (`Z4`–`Z10`)**, **Overlay Opacity (`35%`–`100%`)**, **Play/Pause** animation, and a **Timeline Frame Scrubber**.
  5. **Air Quality & Pollen (`air-quality`):**
     - **US EPA AQI** and **European EAQI** with health recommendations.
     - **24-Hour US AQI & PM2.5** Cairo trend chart.
     - **Particulates & Trace Gases:** `PM2.5`, `PM10`, `Ozone (O₃)`, `Nitrogen Dioxide (NO₂)`, `Sulfur Dioxide (SO₂)`, `Carbon Monoxide (CO)`, `Saharan/Mineral Dust`, and `Aerosol Optical Depth (AOD)`.
     - **Allergens & Pollen (6 species):** `Grass`, `Birch`, `Alder`, `Olive`, `Ragweed`, and `Mugwort` (`grains/m³`).
     - **UV & Clear-Sky UV Index** with protection guidance.
  6. **Sun, Moon & Models (`astronomy`):**
     - **Sun & Daylight Trajectory:** Cairo solar elevation arc with live sun position, golden hour, daylight/sunshine duration, and daily daylight delta.
     - **Moon Phase & Lunar Cycle:** Cairo illuminated lunar disc with phase name, illumination %, synodic cycle age, and countdowns to next Full Moon and New Moon.
     - **Multi-Model Forecast Comparison:** Compares **Open-Meteo Best Match**, **ECMWF IFS (0.25°)**, **NOAA GFS Seamless**, and **DWD ICON Seamless** with a multi-series Cairo chart and model spread (confidence) indicator.

## CLI & Waybar Usage

```bash
# Open or focus Weather
nixe-weather

# Toggle window visibility (ideal for Waybar on-click)
nixe-weather --toggle

# Open directly to a specific section (overview, hourly, daily, radar, air-quality, astronomy, settings)
nixe-weather --section radar

# Open and switch to (or geocode & save) a specific city
nixe-weather --location "Tokyo"

# Output Waybar JSON from SQLite cache (auto-fetches only if cache is stale)
nixe-weather --waybar

# Refresh all saved locations in the SQLite cache and exit
nixe-weather --refresh
```

## Configuration & Cache Paths

- **Settings:** `~/.config/nixe-weather/settings.toml`
- **SQLite Forecast & Radar Tile Cache:** `~/.cache/nixe-weather/cache.db`
- **Unix Control Socket (Glimpse / Calendar / D-Bus bridge):** `$XDG_RUNTIME_DIR/nixe/nixe-weather.sock`

## Attribution & Licensing

- **Weather Forecast, Air Quality & Geocoding:** [Open-Meteo.com](https://open-meteo.com/) under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). Free hosted endpoints are strictly for non-commercial use; configure a self-hosted or commercial API key in `Settings → Units & forecast` if required.
- **Radar & Infrared Satellite Tiles:** [RainViewer API](https://www.rainviewer.com/api.html).
- **Basemap Tiles:** [CARTO](https://carto.com/attributions) & [OpenStreetMap](https://www.openstreetmap.org/copyright) contributors.
- **US Severe Weather Alerts:** [NOAA / National Weather Service API](https://www.weather.gov/documentation/services-web-api) (Public Domain).
