//! Radar & Overlays section: live RainViewer Doppler radar + nowcast, infrared satellite,
//! animated wind streamlines, and temperature/pressure isobar contours.

use crate::widgets::{self, Page};
use crate::{backend, charts, cmd, prefs, theme};
use chrono::{TimeZone, Utc};
use gtk::glib;
use gtk::prelude::*;
use nixe_weather_core::{
    ForecastSnapshot, RadarFrame, RadarLayerKind, RadarManifest, TileGridPayload,
    cardinal_direction,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

#[derive(Default)]
struct RadarViewState {
    layer: RadarLayerKind,
    zoom: u8,
    opacity: f64,
    manifest: Option<RadarManifest>,
    frame_idx: usize,
    playing: bool,
    anim_phase: f64,
    pinch_scale: f64,
    grid: Option<TileGridPayload>,
}

pub fn build(
    page: &Page,
    snap: Option<&ForecastSnapshot>,
    _stale_fallback: bool,
    _last_error: Option<&str>,
) {
    let p = prefs::get();
    let units = p.units();
    let loc = snap
        .map(|s| s.location.clone())
        .or_else(|| p.active_location());

    let Some(loc) = loc else {
        let g = page.group("Radar & overlays");
        g.add(&widgets::empty_state(
            "mark-location-symbolic",
            "No active location",
            "Select or search for a city to view radar and weather overlays.",
            None,
        ));
        return;
    };

    let state = Rc::new(RefCell::new(RadarViewState {
        layer: RadarLayerKind::from_id(&p.default_radar_layer),
        zoom: 6,
        opacity: (p.radar_opacity_pct.clamp(35, 100) as f64) / 100.0,
        manifest: None,
        frame_idx: 0,
        playing: false,
        anim_phase: 0.0,
        pinch_scale: 1.0,
        grid: None,
    }));

    // 1. Layer & Map Controls Card
    let g_map = page.group("Weather radar & spatial overlays");
    let desc_lbl = widgets::label(state.borrow().layer.description(), "group-note");
    desc_lbl.set_wrap(true);
    g_map
        .wrapper
        .insert_child_after(&desc_lbl, g_map.wrapper.first_child().as_ref());

    let toolbar = widgets::hbox(8);
    toolbar.add_css_class("radar-toolbar");

    // Layer switcher segmented control
    let layer_seg = widgets::segmented(
        &widgets::opts(&[
            ("radar", "Radar"),
            ("satellite", "Satellite"),
            ("wind", "Wind"),
            ("temp", "Temp"),
        ]),
        state.borrow().layer.id(),
        {
            let state = state.clone();
            let desc_lbl = desc_lbl.clone();
            move |id| {
                let kind = RadarLayerKind::from_id(&id);
                prefs::update(|pr| pr.default_radar_layer = id);
                {
                    let mut st = state.borrow_mut();
                    st.layer = kind;
                    if let Some(m) = &st.manifest {
                        let frames = active_frames(m, kind);
                        if !frames.is_empty() {
                            st.frame_idx = frames.len().saturating_sub(1);
                        }
                    }
                }
                desc_lbl.set_text(kind.description());
            }
        },
    );
    toolbar.append(&layer_seg);

    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    toolbar.append(&spacer);

    // Zoom controls
    let zoom_out = gtk::Button::with_label("−");
    zoom_out.add_css_class("chip");
    zoom_out.add_css_class("small");
    zoom_out.set_tooltip_text(Some("Zoom out (or pinch in)"));
    let zoom_lbl = widgets::label("Z6", "mono");
    let zoom_in = gtk::Button::with_label("+");
    zoom_in.add_css_class("chip");
    zoom_in.add_css_class("small");
    zoom_in.set_tooltip_text(Some("Zoom in (or pinch out)"));
    toolbar.append(&zoom_out);
    toolbar.append(&zoom_lbl);
    toolbar.append(&zoom_in);
    g_map.add(&toolbar);

    // 2. Interactive Radar & Overlay Canvas (fills width, maintains 16:11 aspect ratio)
    let canvas = gtk::DrawingArea::new();
    canvas.set_content_height(520);
    canvas.set_hexpand(true);
    canvas.add_css_class("radar-canvas");

    canvas.connect_resize(|area, w, _h| {
        if w > 120 {
            let target_h = ((w as f64) * 0.68).round().clamp(280.0, 980.0) as i32;
            if (area.content_height() - target_h).abs() > 4 {
                let a = area.clone();
                glib::idle_add_local_once(move || a.set_content_height(target_h));
            }
        }
    });

    let c_weak = canvas.downgrade();
    theme::subscribe(&canvas, move || {
        if let Some(c) = c_weak.upgrade() {
            c.queue_draw();
        }
    });

    let snap_owned = snap.cloned();
    {
        let state = state.clone();
        let snap_owned = snap_owned.clone();
        canvas.set_draw_func(move |_, cr, w, h| {
            let st = state.borrow();
            charts::draw_radar_map(
                cr,
                w as f64,
                h as f64,
                st.layer,
                st.grid.as_ref(),
                snap_owned.as_ref(),
                units,
                st.opacity,
                st.anim_phase,
                st.pinch_scale,
            );
        });
    }
    g_map.add(&canvas);

    // 3. Timeline Scrubber & Opacity Bar
    let timeline_row = widgets::hbox(10);
    timeline_row.add_css_class("radar-timeline-row");

    let play_btn = gtk::Button::with_label("▶ Play");
    play_btn.add_css_class("chip");
    play_btn.add_css_class("small");
    timeline_row.append(&play_btn);

    let frame_slider = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 12.0, 1.0);
    frame_slider.set_draw_value(false);
    frame_slider.set_hexpand(true);
    timeline_row.append(&frame_slider);

    let frame_time_lbl = widgets::label("Live", "mono");
    frame_time_lbl.set_width_chars(18);
    frame_time_lbl.set_xalign(1.0);
    timeline_row.append(&frame_time_lbl);

    g_map.add(&timeline_row);

    // Opacity row
    let (opacity_row, _) = widgets::slider_row(
        "Overlay opacity",
        "Blend between the cartographic basemap and the active meteorological layer.",
        (35.0, 100.0, 5.0),
        (state.borrow().opacity * 100.0).round(),
        0,
        "%",
        {
            let state = state.clone();
            let canvas = canvas.clone();
            move |v| {
                let pct = v.round().clamp(35.0, 100.0) as u8;
                prefs::update(|pr| pr.radar_opacity_pct = pct);
                state.borrow_mut().opacity = (pct as f64) / 100.0;
                canvas.queue_draw();
            }
        },
    );
    g_map.add(&opacity_row);

    // Helper closure to fetch the 3×3 tile grid for the current state
    let request_tiles: Rc<dyn Fn()> = {
        let state = state.clone();
        let canvas = canvas.clone();
        let frame_time_lbl = frame_time_lbl.clone();
        let frame_slider = frame_slider.clone();
        let loc = loc.clone();
        let updating_slider = Rc::new(Cell::new(false));

        Rc::new(move || {
            let (layer, zoom, frame_opt, host, total_frames, idx) = {
                let st = state.borrow();
                let (f_opt, host, total, idx) = if let Some(m) = &st.manifest {
                    let frames = active_frames(m, st.layer);
                    let clamped_idx = st.frame_idx.min(frames.len().saturating_sub(1));
                    (
                        frames.get(clamped_idx).cloned(),
                        m.host.clone(),
                        frames.len(),
                        clamped_idx,
                    )
                } else {
                    (None, "https://tilecache.rainviewer.com".to_string(), 0, 0)
                };
                (st.layer, st.zoom, f_opt, host, total, idx)
            };

            if total_frames > 1 {
                updating_slider.set(true);
                frame_slider.set_range(0.0, (total_frames - 1) as f64);
                frame_slider.set_value(idx as f64);
                updating_slider.set(false);
                frame_slider.set_sensitive(matches!(
                    layer,
                    RadarLayerKind::PrecipitationRadar | RadarLayerKind::InfraredSatellite
                ));
            } else {
                frame_slider.set_sensitive(false);
            }

            if let Some(f) = &frame_opt
                && matches!(
                    layer,
                    RadarLayerKind::PrecipitationRadar | RadarLayerKind::InfraredSatellite
                )
            {
                let dt_str = Utc
                    .timestamp_opt(f.time, 0)
                    .single()
                    .map(|dt| dt.format("%H:%M UTC").to_string())
                    .unwrap_or_else(|| "Live".into());
                let tag = if f.is_nowcast { "Nowcast" } else { "Observed" };
                frame_time_lbl.set_text(&format!("{tag} · {dt_str}"));
            } else {
                frame_time_lbl.set_text(layer.label());
            }

            let lat = loc.latitude;
            let lon = loc.longitude;

            // 1. Synchronous SQLite cache lookup (< 1ms): render cached basemap/overlay immediately!
            if let Some(cached) =
                backend::get_cached_radar_grid(lat, lon, zoom, &host, frame_opt.as_ref(), layer)
            {
                let mut st = state.borrow_mut();
                if st.zoom == zoom {
                    st.grid = Some(cached);
                    drop(st);
                    canvas.queue_draw();
                }
            }

            let need_basemap = {
                let st = state.borrow();
                match &st.grid {
                    Some(g) if g.center.z == zoom => g.basemap_tiles.iter().any(|t| t.is_none()),
                    _ => true,
                }
            };

            // 2. Fetch background basemap first (if not already complete in cache) and render immediately
            if need_basemap {
                let state_base = state.clone();
                let canvas_base = canvas.clone();
                cmd::background(
                    move || backend::fetch_basemap_grid(lat, lon, zoom),
                    move |base_grid| {
                        let mut st = state_base.borrow_mut();
                        if st.zoom == zoom {
                            match &mut st.grid {
                                Some(existing) if existing.center.z == zoom => {
                                    existing.basemap_tiles = base_grid.basemap_tiles;
                                }
                                _ => {
                                    st.grid = Some(base_grid);
                                }
                            }
                            drop(st);
                            canvas_base.queue_draw();
                        }
                    },
                );
            }

            // 3. Layer radar / satellite overlay tiles on top of the basemap
            if matches!(
                layer,
                RadarLayerKind::PrecipitationRadar | RadarLayerKind::InfraredSatellite
            ) {
                let state_ov = state.clone();
                let canvas_ov = canvas.clone();
                let frame_time = frame_opt.as_ref().map(|f| f.time);
                let is_nowcast = frame_opt.as_ref().is_some_and(|f| f.is_nowcast);
                cmd::background(
                    move || {
                        backend::fetch_overlay_tiles(
                            lat,
                            lon,
                            zoom,
                            &host,
                            frame_opt.as_ref(),
                            layer,
                        )
                    },
                    move |ov_tiles| {
                        let mut st = state_ov.borrow_mut();
                        if st.zoom == zoom && st.layer == layer {
                            if let Some(existing) = &mut st.grid
                                && existing.center.z == zoom
                            {
                                existing.overlay_tiles = ov_tiles;
                                existing.frame_time = frame_time;
                                existing.is_nowcast = is_nowcast;
                            } else {
                                st.grid = Some(TileGridPayload {
                                    center: nixe_weather_core::lat_lon_to_tile(lat, lon, zoom),
                                    basemap_tiles: vec![None; 9],
                                    overlay_tiles: ov_tiles,
                                    frame_time,
                                    is_nowcast,
                                });
                            }
                            drop(st);
                            canvas_ov.queue_draw();
                        }
                    },
                );
            } else {
                canvas.queue_draw();
            }
        })
    };

    // Wire layer switcher to also trigger tile fetch
    {
        let req = request_tiles.clone();
        let mut child = layer_seg.first_child();
        while let Some(w) = child {
            if let Some(btn) = w.downcast_ref::<gtk::Button>() {
                let req2 = req.clone();
                btn.connect_clicked(move |_| req2());
            }
            child = w.next_sibling();
        }
    }

    // Wire zoom buttons
    {
        let state = state.clone();
        let zoom_lbl = zoom_lbl.clone();
        let req = request_tiles.clone();
        zoom_out.connect_clicked(move |_| {
            let mut st = state.borrow_mut();
            if st.zoom > 4 {
                st.zoom -= 1;
                st.pinch_scale = 1.0;
                zoom_lbl.set_text(&format!("Z{}", st.zoom));
                drop(st);
                req();
            }
        });
    }
    {
        let state = state.clone();
        let zoom_lbl = zoom_lbl.clone();
        let req = request_tiles.clone();
        zoom_in.connect_clicked(move |_| {
            let mut st = state.borrow_mut();
            if st.zoom < 10 {
                st.zoom += 1;
                st.pinch_scale = 1.0;
                zoom_lbl.set_text(&format!("Z{}", st.zoom));
                drop(st);
                req();
            }
        });
    }

    // Wire touchpad / touchscreen pinch-to-zoom (gtk::GestureZoom)
    {
        let zoom_gesture = gtk::GestureZoom::new();
        zoom_gesture.set_propagation_phase(gtk::PropagationPhase::Capture);
        let pinch_base = Rc::new(Cell::new(1.0_f64));

        {
            let state = state.clone();
            let pinch_base = pinch_base.clone();
            zoom_gesture.connect_begin(move |_, _| {
                pinch_base.set(1.0);
                state.borrow_mut().pinch_scale = 1.0;
            });
        }
        {
            let state = state.clone();
            let pinch_base = pinch_base.clone();
            let zoom_lbl = zoom_lbl.clone();
            let req = request_tiles.clone();
            let canvas = canvas.clone();
            zoom_gesture.connect_scale_changed(move |_, scale| {
                let base = pinch_base.get().max(0.05);
                let rel = scale / base;
                let mut stepped = false;
                {
                    let mut st = state.borrow_mut();
                    if rel >= 1.38 && st.zoom < 10 {
                        st.zoom += 1;
                        st.pinch_scale = 1.0;
                        pinch_base.set(scale);
                        zoom_lbl.set_text(&format!("Z{}", st.zoom));
                        stepped = true;
                    } else if rel <= 0.72 && st.zoom > 4 {
                        st.zoom -= 1;
                        st.pinch_scale = 1.0;
                        pinch_base.set(scale);
                        zoom_lbl.set_text(&format!("Z{}", st.zoom));
                        stepped = true;
                    } else {
                        st.pinch_scale = rel.clamp(0.85, 1.85);
                    }
                }
                if stepped {
                    req();
                } else {
                    canvas.queue_draw();
                }
            });
        }
        {
            let state = state.clone();
            let zoom_lbl = zoom_lbl.clone();
            let req = request_tiles.clone();
            let canvas = canvas.clone();
            zoom_gesture.connect_end(move |_, _| {
                let mut stepped = false;
                {
                    let mut st = state.borrow_mut();
                    if st.pinch_scale >= 1.18 && st.zoom < 10 {
                        st.zoom += 1;
                        zoom_lbl.set_text(&format!("Z{}", st.zoom));
                        stepped = true;
                    } else if st.pinch_scale <= 0.88 && st.zoom > 4 {
                        st.zoom -= 1;
                        zoom_lbl.set_text(&format!("Z{}", st.zoom));
                        stepped = true;
                    }
                    st.pinch_scale = 1.0;
                }
                if stepped {
                    req();
                } else {
                    canvas.queue_draw();
                }
            });
        }
        canvas.add_controller(zoom_gesture);
    }

    // Wire Ctrl + scroll wheel / touchpad scroll zoom & double-click zoom on canvas
    {
        let scroll_ctrl = gtk::EventControllerScroll::new(
            gtk::EventControllerScrollFlags::VERTICAL | gtk::EventControllerScrollFlags::DISCRETE,
        );
        scroll_ctrl.set_propagation_phase(gtk::PropagationPhase::Capture);
        let state = state.clone();
        let zoom_lbl = zoom_lbl.clone();
        let req = request_tiles.clone();
        let scroll_accum = Cell::new(0.0_f64);
        scroll_ctrl.connect_scroll(move |ctrl, _dx, dy| {
            let mods = ctrl.current_event_state();
            if !mods.contains(gtk::gdk::ModifierType::CONTROL_MASK) {
                return glib::Propagation::Proceed;
            }
            let acc = scroll_accum.get() + dy;
            if acc <= -0.75 {
                scroll_accum.set(0.0);
                let mut st = state.borrow_mut();
                if st.zoom < 10 {
                    st.zoom += 1;
                    st.pinch_scale = 1.0;
                    zoom_lbl.set_text(&format!("Z{}", st.zoom));
                    drop(st);
                    req();
                }
            } else if acc >= 0.75 {
                scroll_accum.set(0.0);
                let mut st = state.borrow_mut();
                if st.zoom > 4 {
                    st.zoom -= 1;
                    st.pinch_scale = 1.0;
                    zoom_lbl.set_text(&format!("Z{}", st.zoom));
                    drop(st);
                    req();
                }
            } else {
                scroll_accum.set(acc);
            }
            glib::Propagation::Stop
        });
        canvas.add_controller(scroll_ctrl);
    }
    {
        let dbl_click = gtk::GestureClick::new();
        let state = state.clone();
        let zoom_lbl = zoom_lbl.clone();
        let req = request_tiles.clone();
        dbl_click.connect_pressed(move |_, n_press, _, _| {
            if n_press == 2 {
                let mut st = state.borrow_mut();
                if st.zoom < 10 {
                    st.zoom += 1;
                    st.pinch_scale = 1.0;
                    zoom_lbl.set_text(&format!("Z{}", st.zoom));
                    drop(st);
                    req();
                }
            }
        });
        canvas.add_controller(dbl_click);
    }

    // Wire frame slider
    {
        let state = state.clone();
        let req = request_tiles.clone();
        frame_slider.connect_value_changed(move |s| {
            let new_idx = s.value().round().max(0.0) as usize;
            let changed = {
                let mut st = state.borrow_mut();
                if st.frame_idx != new_idx {
                    st.frame_idx = new_idx;
                    true
                } else {
                    false
                }
            };
            if changed {
                req();
            }
        });
    }

    // Wire Play/Pause button
    {
        let state = state.clone();
        let play_btn2 = play_btn.clone();
        let req = request_tiles.clone();
        let canvas2 = canvas.clone();
        play_btn.connect_clicked(move |_| {
            let now_playing = {
                let mut st = state.borrow_mut();
                st.playing = !st.playing;
                st.playing
            };
            play_btn2.set_label(if now_playing { "⏸ Pause" } else { "▶ Play" });
            if now_playing {
                let state_tick = state.clone();
                let req_tick = req.clone();
                let canvas_tick = canvas2.clone();
                let btn_tick = play_btn2.clone();
                glib::timeout_add_local(Duration::from_millis(650), move || {
                    if canvas_tick.root().is_none() {
                        return glib::ControlFlow::Break;
                    }
                    let (still_playing, is_tile_layer) = {
                        let mut st = state_tick.borrow_mut();
                        if !st.playing {
                            btn_tick.set_label("▶ Play");
                            return glib::ControlFlow::Break;
                        }
                        st.anim_phase = (st.anim_phase + 0.14).fract();
                        let tile_layer = matches!(
                            st.layer,
                            RadarLayerKind::PrecipitationRadar | RadarLayerKind::InfraredSatellite
                        );
                        if tile_layer && let Some(m) = &st.manifest {
                            let len = active_frames(m, st.layer).len();
                            if len > 0 {
                                st.frame_idx = (st.frame_idx + 1) % len;
                            }
                        }
                        (st.playing, tile_layer)
                    };
                    if !still_playing {
                        return glib::ControlFlow::Break;
                    }
                    if is_tile_layer {
                        req_tick();
                    } else {
                        canvas_tick.queue_draw();
                    }
                    glib::ControlFlow::Continue
                });
            }
        });
    }

    // Smooth animation timer for WindStreamlines when visible
    {
        let state_wind = state.clone();
        let canvas_wind = canvas.clone();
        glib::timeout_add_local(Duration::from_millis(90), move || {
            if canvas_wind.root().is_none() {
                return glib::ControlFlow::Break;
            }
            let mut st = state_wind.borrow_mut();
            if st.layer == RadarLayerKind::WindStreamlines && !prefs::get().reduce_motion {
                st.anim_phase = (st.anim_phase + 0.035).fract();
                drop(st);
                canvas_wind.queue_draw();
            }
            glib::ControlFlow::Continue
        });
    }

    // Load basemap + any cached tiles immediately, then layer live manifest frames on top
    request_tiles();
    {
        let state_init = state.clone();
        let req_init = request_tiles.clone();
        cmd::background(backend::fetch_radar_manifest, move |res| {
            if let Ok(m) = res {
                let mut st = state_init.borrow_mut();
                let frames = active_frames(&m, st.layer);
                st.frame_idx = frames.len().saturating_sub(1);
                st.manifest = Some(m);
                drop(st);
                req_init();
            }
        });
    }

    // 4. Regional Telemetry Summary Card
    if let Some(s) = snap {
        let g_meta = page.group("Regional telemetry & radar coverage");
        let coords = format!("{:.4}°, {:.4}°", s.location.latitude, s.location.longitude);
        g_meta.add(&widgets::row(
            "Target coordinates",
            "Center of the 3×3 Web Mercator radar & satellite composite.",
            Some(widgets::label(&coords, "mono").upcast_ref()),
        ));
        let wind_vec = format!(
            "{} {} ({}°){}",
            units.wind_speed.format(s.current.wind_speed_kmh),
            cardinal_direction(s.current.wind_direction_deg),
            s.current.wind_direction_deg,
            s.current
                .wind_gusts_kmh
                .map(|g| format!(" · gusts {}", units.wind_speed.format(g)))
                .unwrap_or_default()
        );
        g_meta.add(&widgets::row(
            "Surface wind vector",
            "Used for real-time streamline orientation and isotach spacing.",
            Some(widgets::label(&wind_vec, "mono").upcast_ref()),
        ));
        let precip_now = format!(
            "{} · {}% cloud cover",
            units.precipitation.format(s.current.precipitation_mm),
            s.current.cloud_cover_pct
        );
        g_meta.add(&widgets::row(
            "Surface precipitation & cloud deck",
            "Current liquid-equivalent precipitation rate and total cloud fraction.",
            Some(widgets::label(&precip_now, "mono").upcast_ref()),
        ));
    }
}

fn active_frames(m: &RadarManifest, layer: RadarLayerKind) -> &[RadarFrame] {
    match layer {
        RadarLayerKind::InfraredSatellite => &m.satellite_frames,
        _ => &m.radar_frames,
    }
}
