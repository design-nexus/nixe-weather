//! Cairo charts, astronomy diagrams, and interactive radar/overlay map rendering.
//! Every color is derived from `theme::palette()` and `theme::weather_colors()`
//! so live Omarchy theme changes apply immediately to all graphs and maps.

use crate::theme::{self, Rgb};
use gtk::prelude::*;
use gtk::{cairo, gdk_pixbuf, glib};
use nixe_weather_core::{
    FieldGrid, ForecastSnapshot, MoonPhaseInfo, RadarLayerKind, TileGridPayload, UnitPrefs,
    cardinal_direction,
};
use std::f64::consts::PI;

/// Every chart label is in the suite's monospace face.
const MONO: &str = "JetBrains Mono";

fn set_rgb(cr: &cairo::Context, c: Rgb, a: f64) {
    cr.set_source_rgba(c.0, c.1, c.2, a);
}

#[derive(Clone)]
pub struct ChartSeries {
    pub label: String,
    pub values: Vec<f64>,
    pub dashed: bool,
    /// Color index:
    /// 0 = warm/accent, 1 = rain/cyan, 2 = cold/blue, 3 = wind/green, 4 = uv/magenta, 5 = sun/yellow
    pub tone: usize,
}

fn series_color(tone: usize) -> Rgb {
    let p = theme::palette();
    let wc = theme::weather_colors_for(&p);
    let accent = Rgb::hex(&p.accent);
    match tone {
        0 => wc.warm.mix(accent, 0.35),
        1 => wc.rain,
        2 => wc.cold,
        3 => wc.wind,
        4 => wc.uv,
        _ => wc.sun,
    }
}

/// Build a responsive, theme-reactive Cairo multi-series line & area chart with optional background bars.
pub fn line_chart(
    series: Vec<ChartSeries>,
    x_labels: Vec<String>,
    unit_suffix: String,
    fixed_range: Option<(f64, f64)>,
    bars_0_to_100: Option<Vec<f64>>,
    height: i32,
) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_height(height);
    area.set_hexpand(true);

    let a_weak = area.downgrade();
    theme::subscribe(&area, move || {
        if let Some(a) = a_weak.upgrade() {
            a.queue_draw();
        }
    });

    // The pointer's x while it is over the chart: the nearest point is read out.
    let hover = std::rc::Rc::new(std::cell::Cell::new(None::<f64>));
    let motion = gtk::EventControllerMotion::new();
    {
        let (h, a) = (hover.clone(), area.downgrade());
        motion.connect_motion(move |_, x, _| {
            h.set(Some(x));
            if let Some(a) = a.upgrade() {
                a.queue_draw();
            }
        });
        let (h, a) = (hover.clone(), area.downgrade());
        motion.connect_leave(move |_| {
            h.set(None);
            if let Some(a) = a.upgrade() {
                a.queue_draw();
            }
        });
    }
    area.add_controller(motion);

    area.set_draw_func(move |_, cr, w, h| {
        let w = w as f64;
        let h = h as f64;
        let p = theme::palette();
        let wc = theme::weather_colors_for(&p);
        let text = Rgb::hex(&p.text);
        let dim = Rgb::hex(&p.dim_text);
        let border = Rgb::hex(&p.border);
        let surface = Rgb::hex(&p.surface);

        let pad_l = 48.0;
        let pad_r = 18.0;
        let pad_t = 26.0;
        let pad_b = 26.0;
        let cw = (w - pad_l - pad_r).max(40.0);
        let ch = (h - pad_t - pad_b).max(40.0);

        let (mut min_v, mut max_v) = if let Some((lo, hi)) = fixed_range {
            (lo, hi)
        } else {
            let mut lo = f64::INFINITY;
            let mut hi = f64::NEG_INFINITY;
            for s in &series {
                for &v in &s.values {
                    if v.is_finite() {
                        lo = lo.min(v);
                        hi = hi.max(v);
                    }
                }
            }
            if !lo.is_finite() || !hi.is_finite() {
                (0.0, 10.0)
            } else {
                let span = (hi - lo).max(2.0);
                (lo - span * 0.12, hi + span * 0.15)
            }
        };
        if (max_v - min_v).abs() < 1e-6 {
            max_v += 1.0;
            min_v -= 1.0;
        }

        // Decimals enough to tell the labels apart: a pressure in inHg moves by tenths.
        let span = max_v - min_v;
        let digits = if span < 2.0 { 2 } else if span < 20.0 { 1 } else { 0 };

        // Horizontal grid lines + Y-axis labels
        cr.select_font_face(MONO, cairo::FontSlant::Normal, cairo::FontWeight::Normal);
        cr.set_font_size(10.5);
        for i in 0..=4 {
            let frac = i as f64 / 4.0;
            let y = pad_t + ch * (1.0 - frac);
            set_rgb(cr, border, 0.35);
            cr.set_line_width(1.0);
            cr.move_to(pad_l, y);
            cr.line_to(pad_l + cw, y);
            let _ = cr.stroke();

            let val = min_v + (max_v - min_v) * frac;
            let lbl = format!("{val:.digits$}{unit_suffix}");
            set_rgb(cr, dim, 0.88);
            cr.move_to(6.0, y + 3.5);
            let _ = cr.show_text(&lbl);
        }

        // Optional background bars (e.g. precipitation probability 0..100%)
        if let Some(bars) = &bars_0_to_100
            && !bars.is_empty()
        {
            let n = bars.len().max(1);
            let step = cw / n as f64;
            let bar_w = (step * 0.58).clamp(2.5, 16.0);
            for (i, &pct) in bars.iter().enumerate() {
                if pct <= 1.0 {
                    continue;
                }
                let bh = (pct.clamp(0.0, 100.0) / 100.0) * (ch * 0.68);
                let x = pad_l + (i as f64 + 0.5) * step - bar_w * 0.5;
                let y = pad_t + ch - bh;
                let grad = cairo::LinearGradient::new(0.0, y, 0.0, pad_t + ch);
                grad.add_color_stop_rgba(0.0, wc.rain.0, wc.rain.1, wc.rain.2, 0.46);
                grad.add_color_stop_rgba(1.0, wc.rain.0, wc.rain.1, wc.rain.2, 0.10);
                let _ = cr.set_source(&grad);
                cr.rectangle(x, y, bar_w, bh);
                let _ = cr.fill();
            }
        }

        // X-axis labels
        if !x_labels.is_empty() {
            let n = x_labels.len();
            let stride = (n / 7).max(1);
            set_rgb(cr, dim, 0.88);
            for (i, lbl) in x_labels.iter().enumerate() {
                if i % stride != 0 && i + 1 != n {
                    continue;
                }
                let x = if n > 1 {
                    pad_l + (i as f64 / (n - 1) as f64) * cw
                } else {
                    pad_l + cw * 0.5
                };
                cr.move_to((x - 14.0).clamp(pad_l, pad_l + cw - 30.0), h - 7.0);
                let _ = cr.show_text(lbl);
            }
        }

        // Draw series lines + gradient fill under primary series
        for (s_idx, s) in series.iter().enumerate() {
            let n = s.values.len();
            if n < 2 {
                continue;
            }
            let col = series_color(s.tone);
            let pt = |i: usize| -> (f64, f64) {
                let x = pad_l + (i as f64 / (n - 1) as f64) * cw;
                let v = s.values[i].clamp(min_v, max_v);
                let y = pad_t + ch * (1.0 - (v - min_v) / (max_v - min_v));
                (x, y)
            };

            // Runs of finite values; a missing value (NaN) leaves a gap.
            let mut runs: Vec<(usize, usize)> = Vec::new();
            let mut i = 0;
            while i < n {
                if !s.values[i].is_finite() {
                    i += 1;
                    continue;
                }
                let start = i;
                while i < n && s.values[i].is_finite() {
                    i += 1;
                }
                runs.push((start, i));
            }

            if s_idx == 0 && !s.dashed {
                for &(a, b) in runs.iter().filter(|(a, b)| b - a > 1) {
                    cr.new_path();
                    let (x0, y0) = pt(a);
                    cr.move_to(x0, pad_t + ch);
                    cr.line_to(x0, y0);
                    for i in a + 1..b {
                        let (x, y) = pt(i);
                        cr.line_to(x, y);
                    }
                    let (xn, _) = pt(b - 1);
                    cr.line_to(xn, pad_t + ch);
                    cr.close_path();

                    let grad = cairo::LinearGradient::new(0.0, pad_t, 0.0, pad_t + ch);
                    grad.add_color_stop_rgba(0.0, col.0, col.1, col.2, 0.34);
                    grad.add_color_stop_rgba(1.0, col.0, col.1, col.2, 0.02);
                    let _ = cr.set_source(&grad);
                    let _ = cr.fill();
                }
            }

            cr.new_path();
            for &(a, b) in &runs {
                let (x0, y0) = pt(a);
                cr.move_to(x0, y0);
                for i in a + 1..b {
                    let (x, y) = pt(i);
                    cr.line_to(x, y);
                }
            }
            set_rgb(cr, col, if s.dashed { 0.88 } else { 0.98 });
            cr.set_line_width(if s.dashed { 1.8 } else { 2.5 });
            if s.dashed {
                cr.set_dash(&[5.0, 4.0], 0.0);
            } else {
                cr.set_dash(&[], 0.0);
            }
            let _ = cr.stroke();
            cr.set_dash(&[], 0.0);

            // Data point dots & labels at sparse intervals
            let dot_stride = (n / 8).max(1);
            for i in (0..n).step_by(dot_stride).filter(|&i| s.values[i].is_finite()) {
                let (x, y) = pt(i);
                set_rgb(cr, col, 1.0);
                cr.arc(x, y, 3.2, 0.0, 2.0 * PI);
                let _ = cr.fill();
                if s_idx == 0 {
                    set_rgb(cr, text, 0.92);
                    cr.set_font_size(9.5);
                    cr.move_to(x - 8.0, (y - 7.0).max(12.0));
                    let _ = cr.show_text(&format!("{:.digits$}", s.values[i], digits = digits.saturating_sub(1)));
                }
            }
        }

        // Top-right legend
        let mut lx = pad_l + 6.0;
        cr.set_font_size(10.5);
        for s in &series {
            let col = series_color(s.tone);
            set_rgb(cr, col, 1.0);
            cr.arc(lx + 4.0, 11.0, 4.2, 0.0, 2.0 * PI);
            let _ = cr.fill();
            set_rgb(cr, text, 0.92);
            cr.move_to(lx + 12.0, 14.5);
            let _ = cr.show_text(&s.label);
            lx += (s.label.chars().count() as f64 * 6.5) + 28.0;
        }

        // Hover readout: a line at the nearest point and its values.
        let n = series.iter().map(|s| s.values.len()).max().unwrap_or(0);
        if let Some(hx) = hover.get()
            && n > 1
        {
            let i = (((hx - pad_l) / cw) * (n - 1) as f64).round().clamp(0.0, (n - 1) as f64) as usize;
            let x = pad_l + (i as f64 / (n - 1) as f64) * cw;
            set_rgb(cr, dim, 0.7);
            cr.set_line_width(1.0);
            cr.move_to(x, pad_t);
            cr.line_to(x, pad_t + ch);
            let _ = cr.stroke();

            let mut lines: Vec<(Option<Rgb>, String)> = Vec::new();
            if let Some(l) = x_labels.get(i) {
                lines.push((None, l.clone()));
            }
            for s in &series {
                if let Some(v) = s.values.get(i).filter(|v| v.is_finite()) {
                    lines.push((Some(series_color(s.tone)), format!("{}: {v:.prec$}{unit_suffix}", s.label, prec = digits.max(1))));
                }
            }
            if let Some(pct) = bars_0_to_100.as_ref().and_then(|b| b.get(i)) {
                lines.push((Some(wc.rain), format!("Chance of rain: {pct:.0}%")));
            }
            cr.set_font_size(10.5);
            let widest = lines
                .iter()
                .filter_map(|(_, t)| cr.text_extents(t).ok().map(|e| e.x_advance()))
                .fold(0.0, f64::max);
            let (bw, bh) = (widest + 20.0, lines.len() as f64 * 15.0 + 8.0);
            let bx = if x + 10.0 + bw < w { x + 10.0 } else { (x - 10.0 - bw).max(0.0) };
            let by = pad_t;
            set_rgb(cr, surface, 0.96);
            cr.rectangle(bx, by, bw, bh);
            let _ = cr.fill();
            set_rgb(cr, border, 0.8);
            cr.rectangle(bx + 0.5, by + 0.5, bw - 1.0, bh - 1.0);
            let _ = cr.stroke();
            for (k, (c, t)) in lines.iter().enumerate() {
                let ty = by + 15.0 + k as f64 * 15.0;
                if let Some(c) = c {
                    set_rgb(cr, *c, 1.0);
                    cr.arc(bx + 8.0, ty - 3.5, 3.0, 0.0, 2.0 * PI);
                    let _ = cr.fill();
                }
                set_rgb(cr, text, 0.95);
                cr.move_to(bx + 14.0, ty);
                let _ = cr.show_text(t);
            }
        }
    });

    area
}

/// Solar trajectory & horizon arc diagram for the Astronomy section.
pub fn solar_arc_canvas(
    sunrise_iso: Option<String>,
    sunset_iso: Option<String>,
    current_iso: String,
    is_day: bool,
) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_height(155);
    area.set_hexpand(true);

    let a_weak = area.downgrade();
    theme::subscribe(&area, move || {
        if let Some(a) = a_weak.upgrade() {
            a.queue_draw();
        }
    });

    area.set_draw_func(move |_, cr, w, h| {
        let w = w as f64;
        let h = h as f64;
        let p = theme::palette();
        let wc = theme::weather_colors_for(&p);
        let text = Rgb::hex(&p.text);
        let dim = Rgb::hex(&p.dim_text);
        let border = Rgb::hex(&p.border);

        let parse_mins = |iso: &str| -> Option<f64> {
            let t = iso.split('T').nth(1)?;
            let mut parts = t.split(':');
            let hh: f64 = parts.next()?.parse().ok()?;
            let mm: f64 = parts.next()?.parse().ok()?;
            Some(hh * 60.0 + mm)
        };

        let sr_m = sunrise_iso.as_deref().and_then(parse_mins).unwrap_or(390.0);
        let ss_m = sunset_iso.as_deref().and_then(parse_mins).unwrap_or(1110.0);
        let cur_m = parse_mins(&current_iso).unwrap_or(750.0);

        let pad_x = 36.0;
        let horizon_y = h * 0.73;
        let span_w = (w - pad_x * 2.0).max(80.0);
        let amp = (h * 0.50).max(32.0);

        // Daylight warm gradient fill above horizon
        cr.new_path();
        let steps = 60;
        for i in 0..=steps {
            let frac = i as f64 / steps as f64;
            let x = pad_x + frac * span_w;
            let angle = (frac * 2.0 - 0.5) * PI;
            let y = horizon_y - angle.sin() * amp * 0.72;
            if i == 0 {
                cr.move_to(x, y);
            } else {
                cr.line_to(x, y);
            }
        }
        cr.line_to(pad_x + span_w, horizon_y);
        cr.line_to(pad_x, horizon_y);
        cr.close_path();
        let sky_grad = cairo::LinearGradient::new(0.0, horizon_y - amp, 0.0, horizon_y);
        sky_grad.add_color_stop_rgba(0.0, wc.sun.0, wc.sun.1, wc.sun.2, 0.24);
        sky_grad.add_color_stop_rgba(0.6, wc.warm.0, wc.warm.1, wc.warm.2, 0.10);
        sky_grad.add_color_stop_rgba(1.0, wc.warm.0, wc.warm.1, wc.warm.2, 0.0);
        let _ = cr.set_source(&sky_grad);
        let _ = cr.fill();

        // Horizon line
        set_rgb(cr, border, 0.65);
        cr.set_line_width(1.2);
        cr.set_dash(&[4.0, 4.0], 0.0);
        cr.move_to(pad_x, horizon_y);
        cr.line_to(pad_x + span_w, horizon_y);
        let _ = cr.stroke();
        cr.set_dash(&[], 0.0);

        // Sinusoidal solar curve across 24h
        cr.new_path();
        for i in 0..=steps {
            let frac = i as f64 / steps as f64;
            let x = pad_x + frac * span_w;
            let angle = (frac * 2.0 - 0.5) * PI;
            let y = horizon_y - angle.sin() * amp * 0.72;
            if i == 0 {
                cr.move_to(x, y);
            } else {
                cr.line_to(x, y);
            }
        }
        set_rgb(cr, wc.sun, 0.92);
        cr.set_line_width(2.4);
        let _ = cr.stroke();

        // Current sun/moon marker on curve
        let day_span = (ss_m - sr_m).max(60.0);
        let frac = if cur_m >= sr_m && cur_m <= ss_m {
            0.25 + 0.5 * ((cur_m - sr_m) / day_span)
        } else if cur_m < sr_m {
            0.25 * (cur_m / sr_m.max(1.0))
        } else {
            0.75 + 0.25 * ((cur_m - ss_m) / (1440.0 - ss_m).max(1.0))
        }
        .clamp(0.02, 0.98);

        let sx = pad_x + frac * span_w;
        let sy = horizon_y - ((frac * 2.0 - 0.5) * PI).sin() * amp * 0.72;

        let orb_col = if is_day { wc.sun } else { wc.uv };
        set_rgb(cr, orb_col, 0.26);
        cr.arc(sx, sy, 14.0, 0.0, 2.0 * PI);
        let _ = cr.fill();
        set_rgb(cr, orb_col, 1.0);
        cr.arc(sx, sy, 6.2, 0.0, 2.0 * PI);
        let _ = cr.fill();

        // Labels for Sunrise, Solar Noon, Sunset
        cr.select_font_face(MONO, cairo::FontSlant::Normal, cairo::FontWeight::Normal);
        cr.set_font_size(10.5);
        set_rgb(cr, dim, 0.92);
        cr.move_to(pad_x + span_w * 0.20, horizon_y + 18.0);
        let _ = cr.show_text("Sunrise");
        set_rgb(cr, wc.sun, 0.95);
        cr.move_to(pad_x + span_w * 0.45, horizon_y - amp * 0.76 - 8.0);
        let _ = cr.show_text("Solar Noon");
        set_rgb(cr, dim, 0.92);
        cr.move_to(pad_x + span_w * 0.70, horizon_y + 18.0);
        let _ = cr.show_text("Sunset");

        set_rgb(cr, text, 0.92);
        cr.move_to(12.0, 18.0);
        let _ = cr.show_text(if is_day {
            "☀ Sun above horizon"
        } else {
            "☾ Sun below horizon (Night)"
        });
    });

    area
}

/// Draw a realistic illuminated lunar phase disc using Cairo.
pub fn moon_phase_canvas(info: MoonPhaseInfo) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(110);
    area.set_content_height(110);

    let a_weak = area.downgrade();
    theme::subscribe(&area, move || {
        if let Some(a) = a_weak.upgrade() {
            a.queue_draw();
        }
    });

    area.set_draw_func(move |_, cr, w, h| {
        let cx = w as f64 * 0.5;
        let cy = h as f64 * 0.5;
        let r = (w.min(h) as f64 * 0.38).max(20.0);
        let p = theme::palette();
        let wc = theme::weather_colors_for(&p);
        let surface = Rgb::hex(&p.surface);
        let border = Rgb::hex(&p.border);
        let lit_col = wc.sun.mix(Rgb::hex(&p.text), 0.35);

        // Outer ambient lunar glow
        set_rgb(cr, wc.uv, 0.14);
        cr.arc(cx, cy, r + 8.0, 0.0, 2.0 * PI);
        let _ = cr.fill();

        // Dark lunar shadow disc
        set_rgb(cr, surface, 0.95);
        cr.arc(cx, cy, r, 0.0, 2.0 * PI);
        let _ = cr.fill();

        // Lit portion of the moon
        let phase = info.phase_fraction.clamp(0.0, 1.0);
        let waxing = phase <= 0.5;
        let k = (phase * 2.0 * PI).cos(); // +1 at new, -1 at full

        cr.save().ok();
        cr.arc(cx, cy, r, 0.0, 2.0 * PI);
        cr.clip();

        set_rgb(cr, lit_col, 0.92);
        let steps = 48;
        cr.new_path();
        for i in 0..=steps {
            let theta = -PI * 0.5 + (i as f64 / steps as f64) * PI;
            let y = cy + r * theta.sin();
            let x_limb = r * theta.cos();
            let x = if waxing { cx + x_limb } else { cx - x_limb };
            if i == 0 {
                cr.move_to(x, y);
            } else {
                cr.line_to(x, y);
            }
        }
        for i in (0..=steps).rev() {
            let theta = -PI * 0.5 + (i as f64 / steps as f64) * PI;
            let y = cy + r * theta.sin();
            let x_term = r * theta.cos() * k;
            let x = if waxing { cx + x_term } else { cx - x_term };
            cr.line_to(x, y);
        }
        cr.close_path();
        let _ = cr.fill();
        cr.restore().ok();

        // Outer ring
        set_rgb(cr, border, 0.7);
        cr.set_line_width(1.5);
        cr.arc(cx, cy, r, 0.0, 2.0 * PI);
        let _ = cr.stroke();
    });

    area
}

fn decode_png_tile(bytes: &[u8]) -> Option<gdk_pixbuf::Pixbuf> {
    let gbytes = glib::Bytes::from(bytes);
    let stream = gtk::gio::MemoryInputStream::from_bytes(&gbytes);
    gdk_pixbuf::Pixbuf::from_stream(&stream, None::<&gtk::gio::Cancellable>).ok()
}

/// Recolor an OpenStreetMap basemap tile on the fly so it matches the active
/// Omarchy / Nixe theme (dark or light) and changes live when the theme changes.
fn theme_tint_basemap_pixbuf(src: &gdk_pixbuf::Pixbuf) -> gdk_pixbuf::Pixbuf {
    let p = theme::palette();
    let wc = theme::weather_colors_for(&p);
    let bg = Rgb::hex(&p.bg);
    let surface = Rgb::hex(&p.surface);
    let text = Rgb::hex(&p.text);
    let dim = Rgb::hex(&p.dim_text);

    let out = src.copy().unwrap_or_else(|| src.clone());
    let w = out.width() as usize;
    let h = out.height() as usize;
    let rowstride = out.rowstride() as usize;
    let n_channels = out.n_channels() as usize;
    if n_channels < 3 {
        return out;
    }

    // Precompute 0..255 RGB tuples for fast pixel blending
    let to_u8 = |c: Rgb| -> (f64, f64, f64) { (c.0 * 255.0, c.1 * 255.0, c.2 * 255.0) };
    let land_dark = to_u8(surface.mix(bg, 0.35));
    let land_hi = to_u8(surface.mix(text, 0.07));
    let water_col = to_u8(if p.light {
        bg.mix(wc.rain, 0.32)
    } else {
        bg.mix(wc.rain, 0.25)
    });
    let park_col = to_u8(if p.light {
        bg.mix(wc.wind, 0.22)
    } else {
        surface.mix(wc.wind, 0.16)
    });
    let road_col = to_u8(if p.light {
        surface.mix(wc.warm, 0.42)
    } else {
        surface.mix(wc.warm, 0.32)
    });
    let label_col = to_u8(text.mix(dim, 0.15));
    let border_col = to_u8(dim.mix(surface, 0.35));

    // SAFETY: `out` is a freshly cloned unshared Pixbuf and we stay within `h * rowstride`.
    let pixels = unsafe { out.pixels() };
    for y in 0..h {
        let row_start = y * rowstride;
        for x in 0..w {
            let idx = row_start + x * n_channels;
            if idx + 2 >= pixels.len() {
                continue;
            }
            let r = pixels[idx] as f64;
            let g = pixels[idx + 1] as f64;
            let b = pixels[idx + 2] as f64;
            let luma = (0.299 * r + 0.587 * g + 0.114 * b) / 255.0;

            let is_water = b > r + 14.0 && b > g + 4.0;
            let is_park = g > r + 9.0 && g > b + 9.0 && luma > 0.55;
            let is_road = r > b + 20.0 && r >= g && luma > 0.52 && luma < 0.93;
            let is_label_or_line = luma < 0.62;

            let (nr, ng, nb) = if is_water {
                water_col
            } else if is_park {
                park_col
            } else if is_road {
                road_col
            } else if !p.light {
                if is_label_or_line {
                    // Invert dark OSM text/borders into crisp theme text
                    let strength = ((0.62 - luma) / 0.62).clamp(0.0, 1.0);
                    (
                        border_col.0 + (label_col.0 - border_col.0) * strength,
                        border_col.1 + (label_col.1 - border_col.1) * strength,
                        border_col.2 + (label_col.2 - border_col.2) * strength,
                    )
                } else {
                    let t = ((luma - 0.62) / 0.38).clamp(0.0, 1.0);
                    (
                        land_hi.0 + (land_dark.0 - land_hi.0) * t,
                        land_hi.1 + (land_dark.1 - land_hi.1) * t,
                        land_hi.2 + (land_dark.2 - land_hi.2) * t,
                    )
                }
            } else {
                // Light theme: harmonize with theme bg/surface while keeping dark labels sharp
                let bg_u8 = to_u8(bg);
                (r * 0.55 + bg_u8.0 * 0.45, g * 0.55 + bg_u8.1 * 0.45, b * 0.55 + bg_u8.2 * 0.45)
            };

            pixels[idx] = nr.clamp(0.0, 255.0) as u8;
            pixels[idx + 1] = ng.clamp(0.0, 255.0) as u8;
            pixels[idx + 2] = nb.clamp(0.0, 255.0) as u8;
        }
    }
    out
}

/// Convert grayscale GOES Infrared satellite tiles into a translucent, theme-colored
/// cloud-top overlay so clear ground stays transparent and cloud tops glow in theme colors.
fn colorize_ir_pixbuf(src: &gdk_pixbuf::Pixbuf) -> gdk_pixbuf::Pixbuf {
    let p = theme::palette();
    let wc = theme::weather_colors_for(&p);
    let with_alpha = src.add_alpha(false, 0, 0, 0).unwrap_or_else(|_| src.clone());
    let w = with_alpha.width() as usize;
    let h = with_alpha.height() as usize;
    let rowstride = with_alpha.rowstride() as usize;
    let n_channels = with_alpha.n_channels() as usize;
    if n_channels < 4 {
        return with_alpha;
    }

    let low_cloud = wc.rain;
    let high_cloud = wc.uv.mix(wc.sun, 0.4);

    // SAFETY: `with_alpha` is a newly created Pixbuf owned uniquely here.
    let pixels = unsafe { with_alpha.pixels() };
    for y in 0..h {
        let row_start = y * rowstride;
        for x in 0..w {
            let idx = row_start + x * n_channels;
            if idx + 3 >= pixels.len() {
                continue;
            }
            let r = pixels[idx] as f64;
            let g = pixels[idx + 1] as f64;
            let b = pixels[idx + 2] as f64;
            let luma = (0.299 * r + 0.587 * g + 0.114 * b) / 255.0;

            if luma < 0.36 {
                pixels[idx + 3] = 0;
            } else {
                let norm = ((luma - 0.36) / 0.64).clamp(0.0, 1.0);
                let col = low_cloud.mix(high_cloud, norm);
                pixels[idx] = (col.0 * 255.0).clamp(0.0, 255.0) as u8;
                pixels[idx + 1] = (col.1 * 255.0).clamp(0.0, 255.0) as u8;
                pixels[idx + 2] = (col.2 * 255.0).clamp(0.0, 255.0) as u8;
                pixels[idx + 3] = (norm * 225.0).clamp(30.0, 230.0) as u8;
            }
        }
    }
    with_alpha
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

thread_local! {
    static PIXBUF_CACHE: std::cell::RefCell<std::collections::HashMap<(u64, String, u8), gdk_pixbuf::Pixbuf>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

fn cached_tile_pixbuf(bytes: &[u8], theme_sig: &str, mode: u8) -> Option<gdk_pixbuf::Pixbuf> {
    let key = (hash_bytes(bytes), theme_sig.to_string(), mode);
    if let Some(pb) = PIXBUF_CACHE.with(|c| c.borrow().get(&key).cloned()) {
        return Some(pb);
    }
    let raw = decode_png_tile(bytes)?;
    let processed = match mode {
        1 => theme_tint_basemap_pixbuf(&raw),
        2 => colorize_ir_pixbuf(&raw),
        _ => raw,
    };
    PIXBUF_CACHE.with(|c| {
        let mut map = c.borrow_mut();
        if map.len() >= 144 {
            map.clear();
        }
        map.insert(key, processed.clone());
    });
    Some(processed)
}

/// Interactive Radar, Satellite, Wind Streamlines, and Temperature/Isobar map canvas.
#[allow(clippy::too_many_arguments)]
pub fn draw_radar_map(
    cr: &cairo::Context,
    w: f64,
    h: f64,
    layer: RadarLayerKind,
    grid: Option<&TileGridPayload>,
    field: Option<&FieldGrid>,
    snap: Option<&ForecastSnapshot>,
    units: UnitPrefs,
    opacity: f64,
    anim_phase: f64,
    pinch_scale: f64,
    notice: Option<&str>,
) {
    let p = theme::palette();
    let wc = theme::weather_colors_for(&p);
    let bg = Rgb::hex(&p.bg);
    let surface = Rgb::hex(&p.surface);
    let text = Rgb::hex(&p.text);
    let dim = Rgb::hex(&p.dim_text);
    let accent = Rgb::hex(&p.accent);
    let theme_sig = format!("{}:{}:{}", p.bg, p.surface, p.accent);

    // Base background
    set_rgb(cr, surface.mix(bg, 0.5), 1.0);
    cr.rectangle(0.0, 0.0, w, h);
    let _ = cr.fill();

    let cx = w * 0.5;
    let cy = h * 0.5;
    let layout = grid.map(|g| MapLayout::new(g.center, w, h, pinch_scale));

    // 1. Draw 3×3 slippy map basemap + radar/satellite overlay tiles scaled uniformly
    //    so the map always fills 100% of the canvas width & height while keeping 1:1 aspect ratio.
    if let (Some(g), Some(l)) = (grid, layout) {
        let MapLayout { tile_px, origin_x: center_origin_x, origin_y: center_origin_y, .. } = l;
        let scale = (tile_px + 0.5) / 256.0;

        cr.save().ok();
        cr.rectangle(0.0, 0.0, w, h);
        cr.clip();

        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                let idx = ((dy + 1) * 3 + (dx + 1)) as usize;
                let tx = center_origin_x + (dx as f64) * tile_px;
                let ty = center_origin_y + (dy as f64) * tile_px;

                if let Some(Some(base_bytes)) = g.basemap_tiles.get(idx)
                    && let Some(themed_pb) = cached_tile_pixbuf(base_bytes, &theme_sig, 1)
                {
                    cr.save().ok();
                    cr.translate(tx, ty);
                    cr.scale(scale, scale);
                    cr.set_source_pixbuf(&themed_pb, 0.0, 0.0);
                    let _ = cr.paint();
                    cr.restore().ok();
                }

                if matches!(
                    layer,
                    RadarLayerKind::PrecipitationRadar | RadarLayerKind::InfraredSatellite
                ) && let Some(Some(ov_bytes)) = g.overlay_tiles.get(idx)
                {
                    let ov_mode = if layer == RadarLayerKind::InfraredSatellite {
                        2
                    } else {
                        0
                    };
                    if let Some(ov_pb) = cached_tile_pixbuf(ov_bytes, &theme_sig, ov_mode) {
                        cr.save().ok();
                        cr.translate(tx, ty);
                        cr.scale(scale, scale);
                        cr.set_source_pixbuf(&ov_pb, 0.0, 0.0);
                        let _ = cr.paint_with_alpha(opacity.clamp(0.15, 1.0));
                        cr.restore().ok();
                    }
                }
            }
        }
        cr.restore().ok();
    } else {
        // Subtle cartographic graticule while tiles are loading
        set_rgb(cr, dim, 0.18);
        cr.set_line_width(1.0);
        for x in (0..(w as i32)).step_by(64) {
            cr.move_to(x as f64, 0.0);
            cr.line_to(x as f64, h);
        }
        for y in (0..(h as i32)).step_by(64) {
            cr.move_to(0.0, y as f64);
            cr.line_to(w, y as f64);
        }
        let _ = cr.stroke();
    }

    // 2. Fields drawn from the 7×7 Open-Meteo grid: wind arrows, or temperature shading
    //    with isobars.
    if let (Some(f), Some(l)) = (field, layout) {
        match layer {
            RadarLayerKind::WindStreamlines => draw_wind_field(cr, f, l, wc, units, opacity, anim_phase),
            RadarLayerKind::TemperatureContour => draw_temp_field(cr, f, l, wc, text, units, opacity),
            _ => {}
        }
    }

    // 3. Center location target crosshair & live badge
    set_rgb(cr, wc.sun, 0.30);
    cr.arc(cx, cy, 14.0, 0.0, 2.0 * PI);
    let _ = cr.fill();
    set_rgb(cr, wc.sun, 1.0);
    cr.arc(cx, cy, 5.2, 0.0, 2.0 * PI);
    let _ = cr.fill();
    set_rgb(cr, bg, 1.0);
    cr.set_line_width(1.8);
    cr.arc(cx, cy, 5.2, 0.0, 2.0 * PI);
    let _ = cr.stroke();

    // HUD overlay pill in top-left corner
    if let Some(s) = snap {
        let hud_text = match layer {
            RadarLayerKind::PrecipitationRadar => format!(
                "{} · Precip {}% · {}",
                s.location.name,
                s.hourly
                    .first()
                    .map(|h| h.precipitation_probability_pct)
                    .unwrap_or(0),
                units.precipitation.format(s.current.precipitation_mm)
            ),
            RadarLayerKind::InfraredSatellite => format!(
                "{} · Cloud Cover {}% · Vis {}",
                s.location.name,
                s.current.cloud_cover_pct,
                s.current
                    .visibility_m
                    .map(|v| units.distance.format_m(v))
                    .unwrap_or_else(|| "–".into())
            ),
            RadarLayerKind::WindStreamlines => format!(
                "{} · Wind {} {} ({})",
                s.location.name,
                units.wind_speed.format(s.current.wind_speed_kmh),
                cardinal_direction(s.current.wind_direction_deg),
                s.current
                    .wind_gusts_kmh
                    .map(|g| format!("Gusts {}", units.wind_speed.format(g)))
                    .unwrap_or_else(|| "Steady".into())
            ),
            RadarLayerKind::TemperatureContour => format!(
                "{} · {} (Feels {}) · {}",
                s.location.name,
                units.temperature.format_with_unit(s.current.temperature_c),
                units
                    .temperature
                    .format_with_unit(s.current.apparent_temperature_c),
                s.current
                    .pressure_hpa
                    .map(|p| units.pressure.format(p))
                    .unwrap_or_else(|| "–".into())
            ),
        };

        cr.select_font_face(MONO, cairo::FontSlant::Normal, cairo::FontWeight::Bold);
        cr.set_font_size(11.0);
        let tw = cr.text_extents(&hud_text).map(|e| e.x_advance()).unwrap_or(hud_text.len() as f64 * 6.8);
        set_rgb(cr, bg, 0.84);
        cr.rectangle(12.0, 12.0, tw + 20.0, 28.0);
        let _ = cr.fill();
        set_rgb(cr, accent, 0.45);
        cr.set_line_width(1.0);
        cr.rectangle(12.0, 12.0, tw + 20.0, 28.0);
        let _ = cr.stroke();
        set_rgb(cr, text, 0.96);
        cr.move_to(22.0, 30.0);
        let _ = cr.show_text(&hud_text);
    }

    // A note when the layer has nothing to show here (no radar coverage, field loading).
    if let Some(n) = notice {
        cr.select_font_face(MONO, cairo::FontSlant::Normal, cairo::FontWeight::Normal);
        cr.set_font_size(11.0);
        let tw = cr.text_extents(n).map(|e| e.x_advance()).unwrap_or(0.0);
        let (bx, by) = ((cx - tw / 2.0 - 10.0).max(8.0), h - 72.0);
        set_rgb(cr, bg, 0.88);
        cr.rectangle(bx, by, tw + 20.0, 26.0);
        let _ = cr.fill();
        set_rgb(cr, text, 0.92);
        cr.move_to(bx + 10.0, by + 17.0);
        let _ = cr.show_text(n);
    }

    // Map data credits, which the tile services ask for.
    let credit = match layer {
        RadarLayerKind::PrecipitationRadar | RadarLayerKind::InfraredSatellite => {
            "© OpenStreetMap contributors · Radar: RainViewer, IEM"
        }
        _ => "© OpenStreetMap contributors · Data: Open-Meteo",
    };
    cr.select_font_face(MONO, cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    cr.set_font_size(9.5);
    let tw = cr.text_extents(credit).map(|e| e.x_advance()).unwrap_or(0.0);
    set_rgb(cr, bg, 0.78);
    cr.rectangle(w - tw - 14.0, h - 18.0, tw + 14.0, 18.0);
    let _ = cr.fill();
    set_rgb(cr, dim, 1.0);
    cr.move_to(w - tw - 7.0, h - 5.5);
    let _ = cr.show_text(credit);
}

/// Where the 3×3 tile map sits on the canvas: the centre tile's size and its top-left
/// corner, so a lat/lon can be placed on it.
#[derive(Clone, Copy)]
struct MapLayout {
    center: nixe_weather_core::TileCoord,
    tile_px: f64,
    origin_x: f64,
    origin_y: f64,
}

impl MapLayout {
    fn new(center: nixe_weather_core::TileCoord, w: f64, h: f64, pinch_scale: f64) -> Self {
        let cover_x = (1.0 + center.frac_x).min(2.0 - center.frac_x).max(0.5);
        let cover_y = (1.0 + center.frac_y).min(2.0 - center.frac_y).max(0.5);
        let min_cover_px = ((w * 0.5) / cover_x).max((h * 0.5) / cover_y).max(256.0);
        let tile_px = (min_cover_px * 1.18 * pinch_scale.clamp(0.85, 2.0)).max(min_cover_px).ceil();
        Self {
            center,
            tile_px,
            origin_x: (w * 0.5 - center.frac_x * tile_px).floor(),
            origin_y: (h * 0.5 - center.frac_y * tile_px).floor(),
        }
    }

    /// Canvas position of (`lat`, `lon`).
    fn project(&self, lat: f64, lon: f64) -> (f64, f64) {
        let n = (1u32 << self.center.z) as f64;
        let xf = (lon + 180.0) / 360.0 * n;
        let lat_rad = lat.clamp(-85.0511, 85.0511).to_radians();
        let yf = (1.0 - (lat_rad.tan() + 1.0 / lat_rad.cos()).ln() / PI) / 2.0 * n;
        (
            self.origin_x + (xf - self.center.x as f64) * self.tile_px,
            self.origin_y + (yf - self.center.y as f64) * self.tile_px,
        )
    }

    /// Canvas position of grid position (`gx`, `gy`) of `f`.
    fn project_grid(&self, f: &FieldGrid, gx: f64, gy: f64) -> (f64, f64) {
        let max = (f.size - 1).max(1) as f64;
        let b = f.bounds;
        self.project(b.north + (b.south - b.north) * gy / max, b.west + (b.east - b.west) * gx / max)
    }
}

/// Wind arrows over the map, from the grid interpolated as vectors: each points where
/// the air is going, longer and warmer-coloured the stronger it blows, and drifts along
/// its direction with `anim_phase`.
fn draw_wind_field(
    cr: &cairo::Context,
    f: &FieldGrid,
    l: MapLayout,
    wc: theme::WeatherColors,
    units: UnitPrefs,
    opacity: f64,
    anim_phase: f64,
) {
    let max_speed = f.range(|p| p.wind_speed_kmh).map(|r| r.1).unwrap_or(0.0).max(1.0);
    let steps = 16;
    cr.set_line_width(2.0);
    cr.set_line_cap(cairo::LineCap::Round);
    for r in 0..=steps {
        for c in 0..=steps {
            let max = (f.size - 1) as f64;
            let (gx, gy) = (c as f64 / steps as f64 * max, r as f64 / steps as f64 * max);
            let (u, v) = f.wind(gx, gy);
            let speed = u.hypot(v);
            if !speed.is_finite() || speed < 0.5 {
                continue;
            }
            // Canvas y grows southwards.
            let (dx, dy) = (u / speed, -v / speed);
            let len = 10.0 + 22.0 * (speed / max_speed);
            let (x, y) = l.project_grid(f, gx, gy);
            let drift = (anim_phase + (c * 7 + r * 3) as f64 * 0.13).fract() * 10.0 - 5.0;
            let (sx, sy) = (x + dx * drift - dx * len / 2.0, y + dy * drift - dy * len / 2.0);
            let (ex, ey) = (sx + dx * len, sy + dy * len);
            let col = wc.wind.mix(wc.warm, (speed / max_speed).clamp(0.0, 1.0) * 0.7);
            set_rgb(cr, col, 0.85 * opacity);
            cr.move_to(sx, sy);
            cr.line_to(ex, ey);
            let ang = dy.atan2(dx);
            for side in [2.6, -2.6] {
                cr.move_to(ex, ey);
                cr.line_to(ex + (ang + side).cos() * 6.0, ey + (ang + side).sin() * 6.0);
            }
            let _ = cr.stroke();
        }
    }
    cr.set_line_cap(cairo::LineCap::Butt);
    legend(cr, &format!("Wind 10 m · up to {}", units.wind_speed.format(max_speed)), wc.wind);
}

/// Temperature shading (cold to warm across the map's own range) with sea-level
/// isobars every 2 hPa, labelled.
fn draw_temp_field(
    cr: &cairo::Context,
    f: &FieldGrid,
    l: MapLayout,
    wc: theme::WeatherColors,
    text: Rgb,
    units: UnitPrefs,
    opacity: f64,
) {
    let max = (f.size - 1) as f64;
    if let Some((lo, hi)) = f.range(|p| p.temperature_c) {
        let span = (hi - lo).max(1.0);
        let cells = 24;
        for r in 0..cells {
            for c in 0..cells {
                let (gx0, gy0) = (c as f64 / cells as f64 * max, r as f64 / cells as f64 * max);
                let (gx1, gy1) = ((c + 1) as f64 / cells as f64 * max, (r + 1) as f64 / cells as f64 * max);
                let t = f.sample((gx0 + gx1) / 2.0, (gy0 + gy1) / 2.0, |p| p.temperature_c);
                if !t.is_finite() {
                    continue;
                }
                let frac = ((t - lo) / span).clamp(0.0, 1.0);
                let col = if frac < 0.5 { wc.cold.mix(wc.sun, frac * 2.0) } else { wc.sun.mix(wc.danger, (frac - 0.5) * 2.0) };
                let (x0, y0) = l.project_grid(f, gx0, gy0);
                let (x1, y1) = l.project_grid(f, gx1, gy1);
                set_rgb(cr, col, 0.30 * opacity);
                cr.rectangle(x0, y0, x1 - x0 + 0.5, y1 - y0 + 0.5);
                let _ = cr.fill();
            }
        }
        legend(
            cr,
            &format!("2 m temperature {} – {}", units.temperature.format(lo), units.temperature.format(hi)),
            wc.sun,
        );
    }

    let Some((plo, phi)) = f.range(|p| p.pressure_hpa) else { return };
    cr.select_font_face(MONO, cairo::FontSlant::Normal, cairo::FontWeight::Bold);
    cr.set_font_size(10.0);
    let mut level = (plo / 2.0).ceil() * 2.0;
    while level <= phi {
        let segs = f.contour(level, |p| p.pressure_hpa);
        set_rgb(cr, text, 0.55 * opacity);
        cr.set_line_width(1.3);
        for [a, b] in &segs {
            let (x0, y0) = l.project_grid(f, a.0, a.1);
            let (x1, y1) = l.project_grid(f, b.0, b.1);
            cr.move_to(x0, y0);
            cr.line_to(x1, y1);
        }
        let _ = cr.stroke();
        // Label the isobar once, near its middle.
        if let Some([a, _]) = segs.get(segs.len() / 2) {
            let (x, y) = l.project_grid(f, a.0, a.1);
            set_rgb(cr, text, 0.9);
            cr.move_to(x + 4.0, y - 4.0);
            let _ = cr.show_text(&units.pressure.format(level));
        }
        level += 2.0;
    }
}

/// A one-line key in the map's bottom-left corner.
fn legend(cr: &cairo::Context, label: &str, col: Rgb) {
    let p = theme::palette();
    cr.select_font_face(MONO, cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    cr.set_font_size(10.5);
    let tw = cr.text_extents(label).map(|e| e.x_advance()).unwrap_or(0.0);
    let (x, y) = (12.0, cr.clip_extents().map(|e| e.3).unwrap_or(400.0) - 46.0);
    set_rgb(cr, Rgb::hex(&p.bg), 0.84);
    cr.rectangle(x, y, tw + 30.0, 22.0);
    let _ = cr.fill();
    set_rgb(cr, col, 1.0);
    cr.arc(x + 11.0, y + 11.0, 4.0, 0.0, 2.0 * PI);
    let _ = cr.fill();
    set_rgb(cr, Rgb::hex(&p.text), 0.95);
    cr.move_to(x + 20.0, y + 15.0);
    let _ = cr.show_text(label);
}
