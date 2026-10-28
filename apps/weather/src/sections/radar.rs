//! Radar & Overlays section: RainViewer radar and infrared satellite tiles, and the
//! current wind, temperature and isobars over the map from a grid of Open-Meteo points.

use crate::widgets::{self, Page};
use crate::{backend, charts, cmd, prefs, theme};
use gtk::glib;
use gtk::prelude::*;
use nyx_weather_core::{
    FieldGrid, ForecastSnapshot, RAINVIEWER_MAX_ZOOM, RadarFrame, RadarLayerKind, RadarManifest,
    TileGridPayload, cardinal_direction,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

thread_local! {
    /// The zoom last used, so a refresh (which rebuilds the page) keeps it.
    static ZOOM: Cell<u8> = const { Cell::new(6) };
}

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
    /// The wind/temperature/pressure grid, and the zoom it was fetched for.
    field: Option<(u8, FieldGrid)>,
    /// What the map can't show here, drawn over it.
    notice: Option<String>,
    play_timer: Option<glib::SourceId>,
}

/// True for a tile layer (radar or satellite) rather than a field drawn from points.
fn is_tile_layer(layer: RadarLayerKind) -> bool {
    matches!(layer, RadarLayerKind::PrecipitationRadar | RadarLayerKind::InfraredSatellite)
}

pub fn build(
    page: &Page,
    snap: Option<&ForecastSnapshot>,
    _stale_fallback: bool,
    _last_error: Option<&str>,
) {
    let p = prefs::get();
    let utc_offset = snap.map(|s| s.utc_offset_seconds as i64).unwrap_or(0);
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
        zoom: ZOOM.with(Cell::get),
        opacity: (p.radar_opacity_pct.clamp(35, 100) as f64) / 100.0,
        manifest: None,
        frame_idx: 0,
        playing: false,
        anim_phase: 0.0,
        pinch_scale: 1.0,
        grid: None,
        field: None,
        notice: None,
        play_timer: None,
    }));
    // Radar closer in than RainViewer's range is the NEXRAD mosaic, which covers the US.
    let max_zoom = {
        let us = loc.is_us();
        let state = state.clone();
        move || if state.borrow().layer == RadarLayerKind::PrecipitationRadar && !us { RAINVIEWER_MAX_ZOOM } else { 10 }
    };

    // The page doesn't scroll: the map takes the height the window has left after
    // its controls, so the map, the toolbar and the timeline are always in view.
    page.root.set_vscrollbar_policy(gtk::PolicyType::Never);
    page.body.set_vexpand(true);

    // 1. Layer & Map Controls Card
    let g_map = page.group("Weather radar & spatial overlays");
    g_map.wrapper.set_vexpand(true);
    g_map.list.set_vexpand(true);
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

    // Zoom controls, on the map's top-right corner so the toolbar fits a 460 px window.
    let zoom_out = widgets::icon_button("zoom-out-symbolic", "Zoom out (Ctrl+scroll, or pinch)");
    let zoom_lbl = widgets::label(&format!("Z{}", state.borrow().zoom), "tnum");
    let zoom_in = widgets::icon_button("zoom-in-symbolic", "Zoom in (Ctrl+scroll, double-click, or pinch)");
    let map_zoom = widgets::vbox(0);
    map_zoom.add_css_class("map-zoom");
    map_zoom.set_halign(gtk::Align::End);
    map_zoom.set_valign(gtk::Align::Start);
    map_zoom.append(&zoom_in);
    map_zoom.append(&zoom_lbl);
    map_zoom.append(&zoom_out);

    // Overlay opacity and the details about the place, behind one button so the
    // map keeps the room.
    let details = widgets::vbox(4);
    details.add_css_class("radar-details");
    let details_btn = gtk::MenuButton::new();
    details_btn.set_icon_name("view-more-symbolic");
    details_btn.set_tooltip_text(Some("Overlay opacity and map details"));
    details_btn.add_css_class("flat");
    details_btn.set_valign(gtk::Align::Center);
    let details_pop = gtk::Popover::new();
    details_pop.set_child(Some(&details));
    details_btn.set_popover(Some(&details_pop));
    toolbar.append(&details_btn);
    g_map.add(&toolbar);

    // 2. Interactive Radar & Overlay Canvas: fills the card's width and the height
    // left over; the tiles scale to cover any shape.
    let canvas = gtk::DrawingArea::new();
    canvas.set_content_height(MAP_MIN_HEIGHT);
    canvas.set_hexpand(true);
    canvas.set_vexpand(true);
    canvas.add_css_class("radar-canvas");

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
            let field = st.field.as_ref().filter(|(z, _)| *z == st.zoom).map(|(_, f)| f);
            charts::draw_radar_map(
                cr,
                w as f64,
                h as f64,
                st.layer,
                st.grid.as_ref(),
                field,
                snap_owned.as_ref(),
                units,
                st.opacity,
                st.anim_phase,
                st.pinch_scale,
                st.notice.as_deref(),
            );
        });
    }
    let map = gtk::Overlay::new();
    map.set_child(Some(&canvas));
    map.add_overlay(&map_zoom);
    g_map.add(&map);

    // 3. Timeline Scrubber & Opacity Bar
    let timeline_row = widgets::hbox(10);
    timeline_row.add_css_class("radar-timeline-row");

    let play_btn = widgets::icon_button("media-playback-start-symbolic", "Play the radar loop");
    timeline_row.append(&play_btn);

    let frame_slider = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 12.0, 1.0);
    frame_slider.set_draw_value(false);
    frame_slider.set_hexpand(true);
    timeline_row.append(&frame_slider);

    let frame_time_lbl = widgets::label("Live", "tnum");
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
    details.append(&opacity_row);

    // Helper closure to fetch the 3×3 tile grid for the current state
    let request_tiles: Rc<dyn Fn()> = {
        let state = state.clone();
        let canvas = canvas.clone();
        let frame_time_lbl = frame_time_lbl.clone();
        let frame_slider = frame_slider.clone();
        let loc = loc.clone();
        let updating_slider = Rc::new(Cell::new(false));
        let cfg = p.open_meteo_config();

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
            ZOOM.with(|z| z.set(zoom));

            if total_frames > 1 {
                updating_slider.set(true);
                frame_slider.set_range(0.0, (total_frames - 1) as f64);
                frame_slider.set_value(idx as f64);
                updating_slider.set(false);
                frame_slider.set_sensitive(is_tile_layer(layer));
            } else {
                frame_slider.set_sensitive(false);
            }

            // Above RainViewer's zoom, radar is the latest NEXRAD image (no history).
            let live_only = layer == RadarLayerKind::PrecipitationRadar && zoom > RAINVIEWER_MAX_ZOOM;
            if live_only {
                frame_slider.set_sensitive(false);
                frame_time_lbl.set_text("Latest · NEXRAD");
            } else if let Some(f) = &frame_opt
                && is_tile_layer(layer)
            {
                // In the location's own time.
                let local = nyx_weather_core::local_time(f.time, utc_offset as i32);
                let dt_str = if prefs::get().use_24h_time {
                    local.format("%H:%M").to_string()
                } else {
                    local.format("%-I:%M %p").to_string()
                };
                let tag = if f.is_nowcast { "Nowcast" } else { "Observed" };
                frame_time_lbl.set_text(&format!("{tag} · {dt_str}"));
            } else {
                frame_time_lbl.set_text(layer.label());
            }

            let lat = loc.latitude;
            let lon = loc.longitude;

            // What this layer can't show for this place.
            let sat_fallback = frame_opt.as_ref().is_some_and(|f| f.path.starts_with("iem:"));
            let notice = match layer {
                RadarLayerKind::PrecipitationRadar if zoom > RAINVIEWER_MAX_ZOOM && !loc.is_us() => {
                    Some("Radar this close in covers the US only — zoom out".to_string())
                }
                // The fallback imagery is GOES-East: the Americas.
                RadarLayerKind::InfraredSatellite if sat_fallback && !(-170.0..=-20.0).contains(&lon) => {
                    Some("Satellite imagery isn't available here".to_string())
                }
                _ => None,
            };
            state.borrow_mut().notice = notice;

            // Wind and temperature come from a grid of points over the map.
            if !is_tile_layer(layer) {
                let have = state.borrow().field.as_ref().is_some_and(|(z, _)| *z == zoom);
                if !have {
                    state.borrow_mut().notice = Some("Loading the wind and temperature grid…".into());
                    let (state_f, canvas_f, cfg) = (state.clone(), canvas.clone(), cfg.clone());
                    cmd::background(
                        move || backend::fetch_field(lat, lon, zoom, &cfg),
                        move |res| {
                            let mut st = state_f.borrow_mut();
                            match res {
                                Ok(grid) => {
                                    st.field = Some((zoom, grid));
                                    if !is_tile_layer(st.layer) {
                                        st.notice = None;
                                    }
                                }
                                Err(e) if !is_tile_layer(st.layer) => st.notice = Some(e.to_string()),
                                Err(_) => {}
                            }
                            drop(st);
                            canvas_f.queue_draw();
                        },
                    );
                }
            }

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
            if is_tile_layer(layer) {
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
                        // Only the frame still selected (a quick Play can overtake a fetch).
                        let current = st
                            .manifest
                            .as_ref()
                            .and_then(|m| active_frames(m, st.layer).get(st.frame_idx).map(|f| f.time));
                        if st.zoom == zoom && st.layer == layer && (frame_time.is_none() || current == frame_time) {
                            if ov_tiles.iter().all(Option::is_none) && st.notice.is_none() {
                                st.notice = Some("No radar or satellite tiles for this view".into());
                            }
                            if let Some(existing) = &mut st.grid
                                && existing.center.z == zoom
                            {
                                existing.overlay_tiles = ov_tiles;
                                existing.frame_time = frame_time;
                                existing.is_nowcast = is_nowcast;
                            } else {
                                st.grid = Some(TileGridPayload {
                                    center: nyx_weather_core::lat_lon_to_tile(lat, lon, zoom),
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
        let max_zoom = max_zoom.clone();
        zoom_in.connect_clicked(move |_| {
            let limit = max_zoom();
            let mut st = state.borrow_mut();
            if st.zoom < limit {
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
            let max_zoom = max_zoom.clone();
            zoom_gesture.connect_scale_changed(move |_, scale| {
                let base = pinch_base.get().max(0.05);
                let rel = scale / base;
                let mut stepped = false;
                let limit = max_zoom();
                {
                    let mut st = state.borrow_mut();
                    if rel >= 1.38 && st.zoom < limit {
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
            let max_zoom = max_zoom.clone();
            zoom_gesture.connect_end(move |_, _| {
                let mut stepped = false;
                let limit = max_zoom();
                {
                    let mut st = state.borrow_mut();
                    if st.pinch_scale >= 1.18 && st.zoom < limit {
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
        let max_zoom = max_zoom.clone();
        scroll_ctrl.connect_scroll(move |ctrl, _dx, dy| {
            let mods = ctrl.current_event_state();
            if !mods.contains(gtk::gdk::ModifierType::CONTROL_MASK) {
                return glib::Propagation::Proceed;
            }
            let acc = scroll_accum.get() + dy;
            if acc <= -0.75 {
                scroll_accum.set(0.0);
                let limit = max_zoom();
                let mut st = state.borrow_mut();
                if st.zoom < limit {
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
                let limit = max_zoom();
                let mut st = state.borrow_mut();
                if st.zoom < limit {
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
                // One loop at a time: a quick Pause, Play leaves no second timer behind.
                if let Some(old) = st.play_timer.take() {
                    old.remove();
                }
                st.playing
            };
            set_play_icon(&play_btn2, now_playing);
            if now_playing {
                let state_tick = state.clone();
                let req_tick = req.clone();
                let canvas_tick = canvas2.clone();
                let id = glib::timeout_add_local(Duration::from_millis(650), move || {
                    if canvas_tick.root().is_none() {
                        return glib::ControlFlow::Break;
                    }
                    if !canvas_tick.is_mapped() {
                        return glib::ControlFlow::Continue;
                    }
                    let tile_layer = {
                        let mut st = state_tick.borrow_mut();
                        st.anim_phase = (st.anim_phase + 0.14).fract();
                        let tile_layer = is_tile_layer(st.layer);
                        if tile_layer && let Some(m) = &st.manifest {
                            let len = active_frames(m, st.layer).len();
                            if len > 0 {
                                st.frame_idx = (st.frame_idx + 1) % len;
                            }
                        }
                        tile_layer
                    };
                    if tile_layer {
                        req_tick();
                    } else {
                        canvas_tick.queue_draw();
                    }
                    glib::ControlFlow::Continue
                });
                state.borrow_mut().play_timer = Some(id);
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
            if !canvas_wind.is_mapped() {
                return glib::ControlFlow::Continue;
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

    // 4. Regional telemetry, in the details popover
    if let Some(s) = snap {
        details.append(&widgets::label("REGIONAL TELEMETRY", "group-title"));
        let coords = format!("{:.4}°, {:.4}°", s.location.latitude, s.location.longitude);
        details.append(&widgets::row(
            "Target coordinates",
            "Center of the 3×3 Web Mercator radar and satellite composite.",
            Some(widgets::label(&coords, "tnum").upcast_ref()),
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
        details.append(&widgets::row(
            "Surface wind vector",
            "At the location itself; the Wind layer shows it across the map.",
            Some(widgets::label(&wind_vec, "tnum").upcast_ref()),
        ));
        let precip_now = format!(
            "{} · {}% cloud cover",
            units.precipitation.format(s.current.precipitation_mm),
            s.current.cloud_cover_pct
        );
        details.append(&widgets::row(
            "Surface precipitation & cloud deck",
            "Current liquid-equivalent precipitation rate and total cloud fraction.",
            Some(widgets::label(&precip_now, "tnum").upcast_ref()),
        ));
    }
}

/// The map's smallest height; past this the window, not the map, stops shrinking.
const MAP_MIN_HEIGHT: i32 = 200;

fn set_play_icon(b: &gtk::Button, playing: bool) {
    b.set_icon_name(if playing { "media-playback-pause-symbolic" } else { "media-playback-start-symbolic" });
    b.set_tooltip_text(Some(if playing { "Pause the radar loop" } else { "Play the radar loop" }));
}

fn active_frames(m: &RadarManifest, layer: RadarLayerKind) -> &[RadarFrame] {
    match layer {
        RadarLayerKind::InfraredSatellite => &m.satellite_frames,
        _ => &m.radar_frames,
    }
}
