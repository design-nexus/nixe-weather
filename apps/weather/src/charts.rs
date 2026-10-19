//! Cairo charts, astronomy diagrams, and interactive radar/overlay map rendering.
//! Every color is derived from `theme::palette()` and `theme::weather_colors()`
//! so live Omarchy theme changes apply immediately to all graphs and maps.

use crate::theme::{self, Rgb};
use gtk::prelude::*;
use gtk::{cairo, gdk_pixbuf, glib};
use nixe_weather_core::{
    ForecastSnapshot, MoonPhaseInfo, RadarLayerKind, TileGridPayload, UnitPrefs,
    cardinal_direction,
};
use std::f64::consts::PI;

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

    area.set_draw_func(move |_, cr, w, h| {
        let w = w as f64;
        let h = h as f64;
        let p = theme::palette();
        let wc = theme::weather_colors_for(&p);
        let text = Rgb::hex(&p.text);
        let dim = Rgb::hex(&p.dim_text);
        let border = Rgb::hex(&p.border);

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

        // Horizontal grid lines + Y-axis labels
        cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
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
            let lbl = format!("{val:.0}{unit_suffix}");
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

            if s_idx == 0 && !s.dashed {
                cr.new_path();
                let (x0, y0) = pt(0);
                cr.move_to(x0, pad_t + ch);
                cr.line_to(x0, y0);
                for i in 1..n {
                    let (x, y) = pt(i);
                    cr.line_to(x, y);
                }
                let (xn, _) = pt(n - 1);
                cr.line_to(xn, pad_t + ch);
                cr.close_path();

                let grad = cairo::LinearGradient::new(0.0, pad_t, 0.0, pad_t + ch);
                grad.add_color_stop_rgba(0.0, col.0, col.1, col.2, 0.34);
                grad.add_color_stop_rgba(1.0, col.0, col.1, col.2, 0.02);
                let _ = cr.set_source(&grad);
                let _ = cr.fill();
            }

            cr.new_path();
            let (x0, y0) = pt(0);
            cr.move_to(x0, y0);
            for i in 1..n {
                let (x, y) = pt(i);
                cr.line_to(x, y);
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
            for i in (0..n).step_by(dot_stride) {
                let (x, y) = pt(i);
                set_rgb(cr, col, 1.0);
                cr.arc(x, y, 3.2, 0.0, 2.0 * PI);
                let _ = cr.fill();
                if s_idx == 0 {
                    set_rgb(cr, text, 0.92);
                    cr.set_font_size(9.5);
                    cr.move_to(x - 8.0, (y - 7.0).max(12.0));
                    let _ = cr.show_text(&format!("{:.0}", s.values[i]));
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
            lx += (s.label.len() as f64 * 6.5) + 28.0;
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
        cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
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
    snap: Option<&ForecastSnapshot>,
    units: UnitPrefs,
    opacity: f64,
    anim_phase: f64,
    pinch_scale: f64,
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

    // 1. Draw 3×3 slippy map basemap + radar/satellite overlay tiles scaled uniformly
    //    so the map always fills 100% of the canvas width & height while keeping 1:1 aspect ratio.
    if let Some(g) = grid {
        let cover_x = (1.0 + g.center.frac_x).min(2.0 - g.center.frac_x).max(0.5);
        let cover_y = (1.0 + g.center.frac_y).min(2.0 - g.center.frac_y).max(0.5);
        let min_tile_w = (w * 0.5) / cover_x;
        let min_tile_h = (h * 0.5) / cover_y;
        let min_cover_px = min_tile_w.max(min_tile_h).max(256.0);
        let base_tile_px = min_cover_px * 1.18;
        let tile_px = (base_tile_px * pinch_scale.clamp(0.85, 2.0))
            .max(min_cover_px)
            .ceil();
        let scale = (tile_px + 0.5) / 256.0;
        let center_origin_x = (cx - g.center.frac_x * tile_px).floor();
        let center_origin_y = (cy - g.center.frac_y * tile_px).floor();

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

    // 2. Vector meteorological overlays for Wind Streamlines or Temperature & Pressure Isobars
    if let Some(s) = snap {
        match layer {
            RadarLayerKind::WindStreamlines => {
                let dir_deg = s.current.wind_direction_deg as f64;
                // Meteorological wind direction is where wind blows FROM; convert to flow vector
                let flow_rad = (dir_deg + 90.0).to_radians();
                let vx = flow_rad.cos();
                let vy = flow_rad.sin();

                // Isotach rings around location
                for ring in 1..=3 {
                    let r = ring as f64 * 72.0;
                    set_rgb(cr, wc.wind, 0.24 * opacity);
                    cr.set_line_width(1.2);
                    cr.set_dash(&[6.0, 6.0], 0.0);
                    cr.arc(cx, cy, r, 0.0, 2.0 * PI);
                    let _ = cr.stroke();
                }
                cr.set_dash(&[], 0.0);

                // Animated streamlines & directional arrows
                let step = 58.0;
                cr.set_line_width(2.0);
                for gy in (-4..=4).map(|i| cy + i as f64 * step) {
                    for gx in (-6..=6).map(|i| cx + i as f64 * step) {
                        let shift = ((anim_phase * 28.0) + (gx * 0.03 + gy * 0.02)).rem_euclid(28.0);
                        let sx = gx + vx * (shift - 14.0);
                        let sy = gy + vy * (shift - 14.0);
                        let len = 26.0;
                        let ex = sx + vx * len;
                        let ey = sy + vy * len;

                        set_rgb(cr, wc.wind.mix(wc.rain, 0.35), 0.78 * opacity);
                        cr.move_to(sx, sy);
                        cr.line_to(ex, ey);
                        let _ = cr.stroke();

                        // Arrowhead
                        let head = 6.5;
                        let a1 = flow_rad + 2.6;
                        let a2 = flow_rad - 2.6;
                        cr.move_to(ex, ey);
                        cr.line_to(ex + a1.cos() * head, ey + a1.sin() * head);
                        cr.move_to(ex, ey);
                        cr.line_to(ex + a2.cos() * head, ey + a2.sin() * head);
                        let _ = cr.stroke();
                    }
                }
            }
            RadarLayerKind::TemperatureContour => {
                // Radial thermal field + sea-level pressure isobar contours
                let temp_c = s.current.temperature_c;
                let warm_col = if temp_c >= 25.0 {
                    wc.danger.mix(wc.warm, 0.4)
                } else if temp_c >= 12.0 {
                    wc.sun.mix(wc.warm, 0.4)
                } else {
                    wc.cold.mix(wc.rain, 0.4)
                };

                let rad = cairo::RadialGradient::new(cx, cy, 15.0, cx, cy, w.max(h) * 0.55);
                rad.add_color_stop_rgba(0.0, warm_col.0, warm_col.1, warm_col.2, 0.42 * opacity);
                rad.add_color_stop_rgba(0.6, accent.0, accent.1, accent.2, 0.22 * opacity);
                rad.add_color_stop_rgba(1.0, bg.0, bg.1, bg.2, 0.05 * opacity);
                let _ = cr.set_source(&rad);
                cr.rectangle(0.0, 0.0, w, h);
                let _ = cr.fill();

                let base_p = s.current.pressure_hpa.unwrap_or(1013.2);
                cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
                cr.set_font_size(10.0);
                for i in 1..=4 {
                    let rx = i as f64 * 78.0;
                    let ry = i as f64 * 56.0;
                    cr.save().ok();
                    cr.translate(cx, cy);
                    cr.scale(rx, ry);
                    cr.arc(0.0, 0.0, 1.0, 0.0, 2.0 * PI);
                    cr.restore().ok();
                    set_rgb(cr, text, 0.45 * opacity);
                    cr.set_line_width(1.3);
                    let _ = cr.stroke();

                    let iso_p = base_p + (i as f64 - 2.0) * 2.0;
                    set_rgb(cr, text, 0.88);
                    cr.move_to(cx + 8.0, (cy - ry - 4.0).max(16.0));
                    let _ = cr.show_text(&format!("{iso_p:.0} hPa"));
                }
            }
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
                "{} · Precip {}% · {:.1} mm",
                s.location.name,
                s.hourly
                    .first()
                    .map(|h| h.precipitation_probability_pct)
                    .unwrap_or(0),
                s.current.precipitation_mm
            ),
            RadarLayerKind::InfraredSatellite => format!(
                "{} · Cloud Cover {}% · Vis {}",
                s.location.name,
                s.current.cloud_cover_pct,
                s.current
                    .visibility_m
                    .map(|v| format!("{:.1} km", v / 1000.0))
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
                "{} · {} (Feels {}) · {:.0} hPa",
                s.location.name,
                units.temperature.format_with_unit(s.current.temperature_c),
                units
                    .temperature
                    .format_with_unit(s.current.apparent_temperature_c),
                s.current.pressure_hpa.unwrap_or(1013.0)
            ),
        };

        set_rgb(cr, bg, 0.84);
        cr.rectangle(12.0, 12.0, (hud_text.len() as f64 * 6.8) + 22.0, 28.0);
        let _ = cr.fill();
        set_rgb(cr, accent, 0.45);
        cr.set_line_width(1.0);
        cr.rectangle(12.0, 12.0, (hud_text.len() as f64 * 6.8) + 22.0, 28.0);
        let _ = cr.stroke();
        set_rgb(cr, text, 0.96);
        cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
        cr.set_font_size(11.0);
        cr.move_to(22.0, 30.0);
        let _ = cr.show_text(&hud_text);
    }
}
