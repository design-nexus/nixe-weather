//! Overview section: current conditions hero card, severe-weather alerts,
//! 24-hour strip, 7-day summary, and atmospheric conditions grid.

use crate::sections::{format_clock_label, format_day_name, format_duration_secs, format_sun_time};
use crate::widgets::{self, Page};
use crate::{backend, prefs, window};
use gtk::glib;
use gtk::prelude::*;
use nyx_weather_core::{
    ForecastSnapshot, beaufort_scale, cardinal_direction, us_aqi_category, uv_category,
};

pub fn build(
    page: &Page,
    snap: Option<&ForecastSnapshot>,
    stale_fallback: bool,
    last_error: Option<&str>,
) {
    let p = prefs::get();
    let units = p.units();

    let Some(snap) = snap else {
        super::no_forecast(
            page,
            "Current conditions",
            "weather-overcast-symbolic",
            "Loading weather forecast…",
            "Fetching current conditions, hourly forecast and air quality from Open-Meteo.",
            last_error,
        );
        return;
    };

    // Severe-weather alerts (shown only when active)
    for alert in &snap.alerts {
        page.body.append(&alert_banner(alert, units.use_24h_time));
    }
    if snap.alerts_unavailable {
        page.body.append(&widgets::banner("Severe-weather alerts are unavailable right now; NOAA didn't answer.", false));
    }

    if stale_fallback {
        let msg = format!(
            "{} — showing the forecast from {}.",
            last_error.unwrap_or("Offline"),
            backend::human_age(snap.fetched_at)
        );
        page.body.append(&super::retry_banner(&msg));
    }

    // 1. Current Conditions Hero Card
    let cur = &snap.current;
    let (hi_c, lo_c) = snap.today_high_low_c();

    let hero = widgets::vbox(10);
    hero.add_css_class("hero-card");

    let loc_header = widgets::hbox(8);
    let loc_titles = widgets::vbox(2);
    loc_titles.set_hexpand(true);
    let loc_title = widgets::label(&snap.location.name, "hero-location");
    loc_title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    loc_titles.append(&loc_title);
    let sub = snap.location.subtitle();
    if !sub.is_empty() {
        let sub_lbl = widgets::label(&sub, "hero-subtitle");
        sub_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
        loc_titles.append(&sub_lbl);
    }
    loc_header.append(&loc_titles);

    // Quick jumps to Radar and Hourly Graphs
    let jump_box = widgets::hbox(2);
    let radar_btn = widgets::icon_button("mark-location-symbolic", "Radar (Ctrl+4)");
    radar_btn.connect_clicked(|_| window::navigate("radar"));
    jump_box.append(&radar_btn);

    let graphs_btn = widgets::icon_button("view-continuous-symbolic", "Hourly graphs (Ctrl+2)");
    graphs_btn.connect_clicked(|_| window::navigate("hourly"));
    jump_box.append(&graphs_btn);

    let is_saved = p.saved_locations.iter().any(|l| l.id == snap.location.id);
    if !is_saved {
        let save_btn = widgets::icon_button("list-add-symbolic", "Save this location");
        let loc_clone = snap.location.clone();
        save_btn.connect_clicked(move |_| window::save_and_select_location(loc_clone.clone()));
        jump_box.append(&save_btn);
    }
    loc_header.append(&jump_box);
    hero.append(&loc_header);

    let main_row = widgets::hbox(16);
    main_row.set_valign(gtk::Align::Center);
    let temp_lbl = widgets::label(
        &units.temperature.format(cur.temperature_c),
        "hero-temp",
    );
    main_row.append(&temp_lbl);

    let cond_col = widgets::vbox(4);
    cond_col.set_valign(gtk::Align::Center);
    cond_col.set_hexpand(true);
    let cond_row = widgets::hbox(8);
    let cond_icon = gtk::Image::from_icon_name(cur.condition.icon_name(cur.is_day));
    cond_icon.set_pixel_size(26);
    cond_icon.add_css_class(crate::theme::condition_color_class(cur.condition, cur.is_day));
    cond_row.append(&cond_icon);
    let cond_lbl = widgets::label(cur.condition.label(), "hero-condition");
    cond_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
    cond_row.append(&cond_lbl);
    cond_col.append(&cond_row);

    let meta_row = widgets::hbox(10);
    let feels_lbl = widgets::label(
        &format!("Feels like {}", units.temperature.format(cur.apparent_temperature_c)),
        "hero-meta",
    );
    feels_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
    meta_row.append(&feels_lbl);
    meta_row.append(&widgets::label(
        &format!("↑ {}", units.temperature.format(hi_c)),
        "hero-high",
    ));
    meta_row.append(&widgets::label(
        &format!("↓ {}", units.temperature.format(lo_c)),
        "hero-low",
    ));
    cond_col.append(&meta_row);
    if let Some(rain) = snap.rain_nowcast() {
        let rain_lbl = widgets::label(&rain, "hero-nowcast");
        rain_lbl.add_css_class("cond-rain");
        cond_col.append(&rain_lbl);
    }
    main_row.append(&cond_col);
    hero.append(&main_row);

    // Quick metric strip at bottom of hero card
    let quick_strip = widgets::hbox(14);
    quick_strip.add_css_class("hero-quick-strip");
    let wind_lbl = widgets::label(
        &format!(
            "Wind {} {}",
            units.wind_speed.format(cur.wind_speed_kmh),
            cardinal_direction(cur.wind_direction_deg)
        ),
        "hero-stat",
    );
    wind_lbl.add_css_class("hero-stat-wind");
    quick_strip.append(&wind_lbl);

    let hum_lbl = widgets::label(&format!("Humidity {}%", cur.humidity_pct), "hero-stat");
    hum_lbl.add_css_class("hero-stat-rain");
    quick_strip.append(&hum_lbl);

    if let Some(pres) = cur.pressure_hpa {
        let p_lbl = widgets::label(&units.pressure.format(pres), "hero-stat");
        p_lbl.add_css_class("hero-stat-pressure");
        quick_strip.append(&p_lbl);
    }
    if let Some(aqi) = snap.air_quality.as_ref().and_then(|a| a.us_aqi) {
        let a_lbl = widgets::label(
            &format!("AQI {} ({})", aqi, us_aqi_category(aqi)),
            "hero-stat",
        );
        a_lbl.add_css_class("hero-stat-uv");
        quick_strip.append(&a_lbl);
    }
    let quick_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::External)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .hexpand(true)
        .child(&quick_strip)
        .build();
    hero.append(&quick_scroll);
    page.body.append(&hero);

    // 2. Horizontally scrollable 24-hour forecast (scrollbar hidden)
    let g_hourly = page.group("24-hour forecast");
    let h_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::External)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .hexpand(true)
        .build();
    h_scroll.add_css_class("hourly-scroller");
    {
        let hs = h_scroll.clone();
        let wheel = gtk::EventControllerScroll::new(
            gtk::EventControllerScrollFlags::BOTH_AXES | gtk::EventControllerScrollFlags::KINETIC,
        );
        wheel.connect_scroll(move |_, dx, dy| {
            let delta = if dx.abs() > 0.01 { dx } else { dy };
            if delta.abs() > 0.01 {
                let adj = hs.hadjustment();
                let next = (adj.value() + delta * 42.0)
                    .clamp(adj.lower(), (adj.upper() - adj.page_size()).max(adj.lower()));
                adj.set_value(next);
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        h_scroll.add_controller(wheel);
    }

    let h_row = widgets::hbox(0);
    h_row.add_css_class("hourly-strip");
    let upcoming = snap.upcoming_hours(24);
    for (idx, h) in upcoming.iter().enumerate() {
        let cell = widgets::vbox(6);
        cell.add_css_class("hourly-cell");
        if idx == 0 {
            cell.add_css_class("now");
        }
        let time_str = if idx == 0 {
            "Now".to_string()
        } else {
            format_clock_label(&h.time, units.use_24h_time)
        };
        let t_lbl = widgets::label(&time_str, "hourly-time");
        t_lbl.set_halign(gtk::Align::Center);
        cell.append(&t_lbl);

        let icon = gtk::Image::from_icon_name(h.condition.icon_name(h.is_day));
        icon.set_pixel_size(20);
        icon.set_halign(gtk::Align::Center);
        icon.add_css_class(crate::theme::condition_color_class(h.condition, h.is_day));
        cell.append(&icon);

        let pop_str = if h.precipitation_probability_pct >= 10 {
            format!("{}%", h.precipitation_probability_pct)
        } else {
            " ".to_string()
        };
        let pop_lbl = widgets::label(&pop_str, "hourly-pop");
        pop_lbl.set_halign(gtk::Align::Center);
        cell.append(&pop_lbl);

        let temp_lbl = widgets::label(&units.temperature.format(h.temperature_c), "hourly-temp");
        temp_lbl.set_halign(gtk::Align::Center);
        cell.append(&temp_lbl);

        h_row.append(&cell);
    }
    h_scroll.set_child(Some(&h_row));
    g_hourly.add(&h_scroll);

    // 3. 7-Day Forecast with temperature range bars & precipitation probability
    let g_daily = page.group("7-day forecast");
    let more_days = widgets::icon_button("go-next-symbolic", "10-day outlook (Ctrl+3)");
    more_days.connect_clicked(|_| window::navigate("daily"));
    g_daily.header_end(&more_days);

    let week_min = snap
        .daily
        .iter()
        .take(7)
        .map(|d| d.temp_min_c)
        .fold(f64::INFINITY, f64::min);
    let week_max = snap
        .daily
        .iter()
        .take(7)
        .map(|d| d.temp_max_c)
        .fold(f64::NEG_INFINITY, f64::max);
    let week_span = (week_max - week_min).max(1.0);

    for (idx, d) in snap.daily.iter().take(7).enumerate() {
        let d_row = widgets::hbox(10);
        d_row.add_css_class("daily-row");

        let day_name = format_day_name(&d.date, idx);
        let day_lbl = widgets::label(&day_name, "daily-day");
        day_lbl.set_width_chars(5);
        d_row.append(&day_lbl);

        let icon = gtk::Image::from_icon_name(d.condition.icon_name(true));
        icon.set_pixel_size(18);
        icon.add_css_class(crate::theme::condition_color_class(d.condition, true));
        d_row.append(&icon);

        let cond_lbl = widgets::label(d.condition.label(), "dim");
        cond_lbl.set_hexpand(true);
        cond_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
        d_row.append(&cond_lbl);

        let pop_text = if d.precipitation_probability_max_pct >= 10 {
            format!("{}%", d.precipitation_probability_max_pct)
        } else {
            String::new()
        };
        let pop_lbl = widgets::label(&pop_text, "daily-pop");
        pop_lbl.set_width_chars(4);
        pop_lbl.set_xalign(1.0);
        d_row.append(&pop_lbl);

        let min_lbl = widgets::label(&units.temperature.format(d.temp_min_c), "daily-min");
        min_lbl.set_width_chars(4);
        min_lbl.set_xalign(1.0);
        d_row.append(&min_lbl);

        let bar_track = super::range_bar(d.temp_min_c, d.temp_max_c, week_min, week_span, 110);
        d_row.append(&bar_track);

        let max_lbl = widgets::label(&units.temperature.format(d.temp_max_c), "daily-max");
        max_lbl.set_width_chars(4);
        max_lbl.set_xalign(1.0);
        d_row.append(&max_lbl);

        g_daily.add(&d_row);
    }

    // 4. Expandable Details for Humidity, Wind, UV Index, Air Quality, and Sunrise/Sunset
    let g_details = page.group("Conditions & details");
    g_details.note("Click a row, or press Enter on it, for more.");

    // Wind
    let (bft_num, bft_label) = beaufort_scale(cur.wind_speed_kmh);
    let wind_summary = format!(
        "{} {} · Bft {bft_num} ({bft_label})",
        units.wind_speed.format(cur.wind_speed_kmh),
        cardinal_direction(cur.wind_direction_deg)
    );
    let gusts_str = cur
        .wind_gusts_kmh
        .map(|g| units.wind_speed.format(g))
        .unwrap_or_else(|| "–".into());
    let daily_max_wind = snap
        .daily
        .first()
        .map(|d| units.wind_speed.format(d.wind_speed_max_kmh))
        .unwrap_or_else(|| "–".into());
    g_details.add(&expandable_detail_row(
        "weather-windy-symbolic",
        "Wind & Beaufort Scale",
        &wind_summary,
        &[
            ("Sustained wind", units.wind_speed.format(cur.wind_speed_kmh)),
            ("Wind gusts", gusts_str),
            (
                "Compass bearing",
                format!(
                    "{}° ({})",
                    cur.wind_direction_deg,
                    cardinal_direction(cur.wind_direction_deg)
                ),
            ),
            ("Beaufort scale", format!("{bft_num} — {bft_label}")),
            ("Today's max wind", daily_max_wind),
        ],
    ));

    // Humidity & Pressure
    let hum_summary = format!(
        "{}% · {}",
        cur.humidity_pct,
        cur.pressure_hpa
            .map(|p| units.pressure.format(p))
            .unwrap_or_else(|| "–".into())
    );
    let dew_str = cur
        .dew_point_c
        .map(|d| units.temperature.format_with_unit(d))
        .unwrap_or_else(|| "–".into());
    let vis_str = cur
        .visibility_m
        .map(|v| units.distance.format_m(v))
        .unwrap_or_else(|| "–".into());
    g_details.add(&expandable_detail_row(
        "weather-showers-scattered-symbolic",
        "Humidity, Pressure & Visibility",
        &hum_summary,
        &[
            ("Relative humidity", format!("{}%", cur.humidity_pct)),
            ("Dew point", dew_str),
            (
                "Surface pressure",
                cur.pressure_hpa
                    .map(|p| units.pressure.format(p))
                    .unwrap_or_else(|| "–".into()),
            ),
            ("Cloud cover", format!("{}%", cur.cloud_cover_pct)),
            ("Visibility", vis_str),
        ],
    ));

    // UV Index
    let uv_summary = cur
        .uv_index
        .map(|u| format!("{u:.1} · {}", uv_category(u)))
        .unwrap_or_else(|| "Unavailable".into());
    let uv_max_today = snap
        .daily
        .first()
        .and_then(|d| d.uv_index_max)
        .map(|u| format!("{u:.1} ({})", uv_category(u)))
        .unwrap_or_else(|| "–".into());
    let uv_advice = match cur.uv_index {
        None => "No current reading.",
        Some(x) if x < 3.0 => "Minimal protection needed for normal outdoor activity.",
        Some(x) if x < 6.0 => "Seek shade near midday; wear sunglasses and SPF 30+.",
        Some(x) if x < 8.0 => "Reduce midday sun exposure; protective clothing recommended.",
        Some(_) => "Avoid midday sun; high UV exposure risk.",
    };
    g_details.add(&expandable_detail_row(
        "weather-clear-symbolic",
        "UV Index & Solar",
        &uv_summary,
        &[
            ("Current UV index", uv_summary.clone()),
            ("Today's max UV", uv_max_today),
            ("Guidance", uv_advice.into()),
        ],
    ));

    // Air Quality
    let aq = snap.air_quality.clone().unwrap_or_default();
    let aqi_summary = aq
        .us_aqi
        .map(|v| format!("US AQI {v} · {}", us_aqi_category(v)))
        .unwrap_or_else(|| "Unavailable".into());
    g_details.add(&expandable_detail_row(
        "weather-fog-symbolic",
        "Air Quality",
        &aqi_summary,
        &[
            (
                "US EPA AQI",
                aq.us_aqi
                    .map(|v| format!("{v} ({})", us_aqi_category(v)))
                    .unwrap_or_else(|| "–".into()),
            ),
            (
                "European EAQI",
                aq.european_aqi
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "–".into()),
            ),
            (
                "PM2.5 (fine particles)",
                aq.pm2_5
                    .map(|v| format!("{v:.1} μg/m³"))
                    .unwrap_or_else(|| "–".into()),
            ),
            (
                "PM10 (coarse particles)",
                aq.pm10
                    .map(|v| format!("{v:.1} μg/m³"))
                    .unwrap_or_else(|| "–".into()),
            ),
            (
                "Ozone (O₃)",
                aq.ozone
                    .map(|v| format!("{v:.1} μg/m³"))
                    .unwrap_or_else(|| "–".into()),
            ),
            (
                "Nitrogen dioxide (NO₂)",
                aq.nitrogen_dioxide
                    .map(|v| format!("{v:.1} μg/m³"))
                    .unwrap_or_else(|| "–".into()),
            ),
        ],
    ));

    // Sunrise / Sunset
    let today_daily = snap.daily.first();
    let sr = format_sun_time(today_daily.and_then(|d| d.sunrise.as_deref()), units.use_24h_time);
    let ss = format_sun_time(today_daily.and_then(|d| d.sunset.as_deref()), units.use_24h_time);
    let sun_summary = format!("↑ {sr}  ·  ↓ {ss}");
    g_details.add(&expandable_detail_row(
        "weather-clear-night-symbolic",
        "Sunrise & Sunset",
        &sun_summary,
        &[
            ("Sunrise", sr),
            ("Sunset", ss),
            (
                "Daylight duration",
                format_duration_secs(today_daily.and_then(|d| d.daylight_duration_secs)),
            ),
            (
                "Sunshine duration",
                format_duration_secs(today_daily.and_then(|d| d.sunshine_duration_secs)),
            ),
            ("Timezone", snap.location.timezone.clone()),
        ],
    ));
}

fn expandable_detail_row(
    icon_name: &str,
    title: &str,
    summary_right: &str,
    sub_rows: &[(&str, String)],
) -> gtk::Box {
    let outer = widgets::vbox(0);
    outer.add_css_class("detail-expand-row");

    let header = widgets::hbox(10);
    let icon = gtk::Image::from_icon_name(icon_name);
    icon.set_pixel_size(17);
    let icon_color = match icon_name {
        "weather-windy-symbolic" => "cond-wind",
        "weather-showers-scattered-symbolic" => "cond-rain",
        "weather-clear-symbolic" => "cond-sun",
        "weather-fog-symbolic" => "cond-uv",
        _ => "cond-warm",
    };
    icon.add_css_class(icon_color);
    header.append(&icon);

    let title_lbl = widgets::label(title, "detail-title");
    title_lbl.set_hexpand(true);
    header.append(&title_lbl);

    let sum_lbl = widgets::label(summary_right, "detail-summary");
    header.append(&sum_lbl);

    let chevron = gtk::Image::from_icon_name("pan-down-symbolic");
    chevron.add_css_class("dim");
    header.append(&chevron);
    let toggle = gtk::Button::builder().child(&header).css_classes(["flat", "detail-expand-header"]).build();
    outer.append(&toggle);

    let body = widgets::vbox(6);
    body.add_css_class("detail-expand-body");
    body.set_visible(false);

    for (k, v) in sub_rows {
        let r = widgets::hbox(8);
        let kl = widgets::label(k, "dim");
        kl.set_hexpand(true);
        let vl = widgets::label(v, "tnum");
        r.append(&kl);
        r.append(&vl);
        body.append(&r);
    }
    outer.append(&body);

    let b2 = body.clone();
    toggle.connect_clicked(move |_| {
        let next = !b2.is_visible();
        b2.set_visible(next);
        chevron.set_icon_name(Some(if next { "pan-up-symbolic" } else { "pan-down-symbolic" }));
    });

    outer
}

/// A severe-weather alert: a warning banner with its event and headline, and its
/// description, instructions, area and times behind a toggle.
fn alert_banner(alert: &nyx_weather_core::WeatherAlert, use_24h: bool) -> gtk::Box {
    let banner = widgets::banner("", true);
    banner.add_css_class("alert-banner");
    // The shared banner is icon + label; put a column of our own in the label's place.
    if let Some(l) = banner.last_child() {
        banner.remove(&l);
    }
    let col = widgets::vbox(4);
    col.set_hexpand(true);
    let title = widgets::label(&format!("{} · {}", alert.event, alert.severity.label()), "alert-title");
    title.set_wrap(true);
    col.append(&title);
    let hl = widgets::label(&alert.headline, "alert-headline");
    hl.set_wrap(true);
    col.append(&hl);

    let details = widgets::vbox(6);
    details.set_visible(false);
    let when = |iso: &Option<String>| {
        iso.as_deref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|t| t.format(if use_24h { "%a %-d %b, %H:%M" } else { "%a %-d %b, %-I:%M %p" }).to_string())
    };
    let mut meta = Vec::new();
    if let Some(t) = when(&alert.effective) {
        meta.push(format!("From {t}"));
    }
    if let Some(t) = when(&alert.expires) {
        meta.push(format!("until {t}"));
    }
    if let Some(sender) = &alert.sender {
        meta.push(sender.clone());
    }
    if !meta.is_empty() {
        details.append(&widgets::label(&meta.join(" · "), "dim"));
    }
    if !alert.area.is_empty() {
        let area = widgets::label(&alert.area, "dim");
        area.set_wrap(true);
        details.append(&area);
    }
    let desc = widgets::label(&alert.description, "alert-body");
    desc.set_wrap(true);
    details.append(&desc);
    if let Some(inst) = &alert.instruction {
        let inst_lbl = widgets::label(&format!("Instruction: {inst}"), "alert-body");
        inst_lbl.set_wrap(true);
        details.append(&inst_lbl);
    }
    col.append(&details);
    banner.append(&col);

    let toggle = widgets::icon_button("pan-down-symbolic", "Show details");
    toggle.set_valign(gtk::Align::Start);
    toggle.connect_clicked(move |b| {
        let open = !details.is_visible();
        details.set_visible(open);
        b.set_icon_name(if open { "pan-up-symbolic" } else { "pan-down-symbolic" });
        b.set_tooltip_text(Some(if open { "Hide details" } else { "Show details" }));
    });
    banner.append(&toggle);
    banner
}
