//! D-Bus control interface (`io.github.design_nexus.nixe.Control`) for Nixe Assistant,
//! Nixe Glimpse, and Nixe Calendar integration.

use crate::{backend, cmd, prefs, window};
use nixe_core::control::{self, Reply};
use nixe_weather_core::{evaluate_query, parse_query};
use serde_json::{Value, json};

pub fn tools() -> Value {
    json!([
        {
            "name": "current",
            "description": "Get the current weather conditions and daily summary for the active or named location",
            "parameters": {
                "location": "Optional city or location name; omit for the active saved location"
            }
        },
        {
            "name": "forecast",
            "description": "Get the 7-day and 24-hour weather forecast for the active or named location (reusable by Calendar)",
            "parameters": {
                "location": "Optional city or location name; omit for the active saved location"
            }
        },
        {
            "name": "query",
            "description": "Answer a natural weather query such as 'weather', 'weather tomorrow', 'rain Friday', or 'weather London'",
            "parameters": {
                "query": "Query string"
            }
        },
        {
            "name": "locations",
            "description": "List saved locations and the currently active location ID"
        },
        {
            "name": "select_location",
            "description": "Switch the active location by name or ID",
            "parameters": {
                "location": "Location name or ID"
            }
        },
        {
            "name": "refresh",
            "description": "Refresh the active location's weather forecast"
        }
    ])
}

pub fn handle(name: &str, args: Value, reply: Reply) {
    match name {
        "locations" => {
            let p = prefs::get();
            reply(Ok(json!({
                "active_location_id": p.active_location_id,
                "saved_locations": p.saved_locations,
            })));
        }
        "select_location" => {
            let Ok(target) = control::text(&args, "location") else {
                reply(Err("location is required".into()));
                return;
            };
            let q = target.trim().to_lowercase();
            let p = prefs::get();
            if let Some(found) = p
                .saved_locations
                .iter()
                .find(|l| l.id == target || l.name.to_lowercase().contains(&q))
                .cloned()
            {
                window::select_location(&found.id);
                reply(Ok(json!({ "selected": found })));
            } else {
                reply(Err(format!("No saved location matches '{target}'")));
            }
        }
        "refresh" => {
            window::refresh_active(true);
            reply(Ok(json!({ "refreshing": true })));
        }
        "current" | "forecast" => {
            let loc_arg = args["location"].as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
            let p = prefs::get();
            let cfg = p.open_meteo_config();
            let max_age = (p.refresh_interval_mins.max(5) as i64) * 60;
            let want_full = name == "forecast";
            let units = p.units();

            cmd::background(
                move || -> Result<Value, String> {
                    let target_loc = if let Some(q) = loc_arg {
                        let q_low = q.to_lowercase();
                        if let Some(saved) = p.saved_locations.iter().find(|l| l.name.to_lowercase().contains(&q_low)) {
                            saved.clone()
                        } else {
                            let found = backend::search_locations(&q, &cfg).map_err(|e| e.to_string())?;
                            found.into_iter().next().ok_or_else(|| format!("Location '{q}' not found"))?
                        }
                    } else {
                        p.active_location().ok_or_else(|| "No active location configured".to_string())?
                    };

                    let outcome = backend::load_or_fetch(&target_loc, &cfg, max_age, false);
                    let Some(snap) = outcome.snapshot else {
                        return Err(outcome.error.map(|e| e.to_string()).unwrap_or_else(|| "Forecast unavailable".into()));
                    };

                    if want_full {
                        Ok(json!({
                            "location": snap.location,
                            "fetched_at": snap.fetched_at,
                            "stale_fallback": outcome.stale_fallback,
                            "current": snap.current,
                            "hourly": snap.upcoming_hours(24),
                            "daily": snap.daily,
                            "air_quality": snap.air_quality,
                            "alerts": snap.alerts,
                            "attribution": snap.attribution,
                        }))
                    } else {
                        let (hi, lo) = snap.today_high_low_c();
                        Ok(json!({
                            "location": snap.location.display_name(),
                            "condition": snap.current.condition.label(),
                            "temperature": units.temperature.format_with_unit(snap.current.temperature_c),
                            "feels_like": units.temperature.format_with_unit(snap.current.apparent_temperature_c),
                            "high": units.temperature.format_with_unit(hi),
                            "low": units.temperature.format_with_unit(lo),
                            "wind": units.wind_speed.format(snap.current.wind_speed_kmh),
                            "humidity_pct": snap.current.humidity_pct,
                            "alerts": snap.alerts,
                            "attribution": snap.attribution.attribution_text,
                        }))
                    }
                },
                reply,
            );
        }
        "query" => {
            let Ok(raw) = control::text(&args, "query") else {
                reply(Err("query is required".into()));
                return;
            };
            let intent = parse_query(&raw).unwrap_or(nixe_weather_core::QueryIntent {
                rain_focus: false,
                day: nixe_weather_core::query::TargetDay::Today,
                location_query: None,
            });
            let p = prefs::get();
            let cfg = p.open_meteo_config();
            let units = p.units();
            let max_age = (p.refresh_interval_mins.max(5) as i64) * 60;

            cmd::background(
                move || -> Result<Value, String> {
                    let target_loc = if let Some(q) = &intent.location_query {
                        let q_low = q.to_lowercase();
                        if let Some(saved) = p.saved_locations.iter().find(|l| l.name.to_lowercase().contains(&q_low)) {
                            saved.clone()
                        } else {
                            let found = backend::search_locations(q, &cfg).map_err(|e| e.to_string())?;
                            found.into_iter().next().ok_or_else(|| format!("Location '{q}' not found"))?
                        }
                    } else {
                        p.active_location().ok_or_else(|| "No active location configured".to_string())?
                    };

                    let outcome = backend::load_or_fetch(&target_loc, &cfg, max_age, false);
                    let Some(snap) = outcome.snapshot else {
                        return Err(outcome.error.map(|e| e.to_string()).unwrap_or_else(|| "Forecast unavailable".into()));
                    };

                    let answer = evaluate_query(&intent, &snap, units);
                    Ok(serde_json::to_value(answer).unwrap_or_else(|_| json!({})))
                },
                reply,
            );
        }
        _ => reply(Err(format!("unknown tool: {name}"))),
    }
}
