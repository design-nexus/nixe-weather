//! Theming: the shared Nyx theme engine, plus rich semantic weather colors
//! derived from Omarchy's `colors.toml` or the active Nyx theme palette.

pub use nyx_theme::*;

use crate::prefs;
use nyx_weather_core::WeatherCondition;

/// Rich semantic colors for weather conditions, charts, and radar maps.
/// Changes automatically whenever the Omarchy theme or Nyx theme changes.
#[derive(Debug, Clone, Copy)]
pub struct WeatherColors {
    /// Yellow / Gold — Sun, clear sky, moderate alerts, daylight arc
    pub sun: Rgb,
    /// Orange / Amber — Warm temperatures, daily highs, elevated pollen/UV
    pub warm: Rgb,
    /// Cyan / Teal — Rain, precipitation probability, humidity, dew point
    pub rain: Rgb,
    /// Green / Mint — Wind, healthy AQI, low UV, parks
    pub wind: Rgb,
    /// Magenta / Violet — UV index, thunderstorms, ensemble models
    pub uv: Rgb,
    /// Blue / Ice — Cold temperatures, daily lows, snow, pressure
    pub cold: Rgb,
    /// Red / Coral — Severe alerts, extreme heat/AQI
    pub danger: Rgb,
}

fn rotate_hue(c: Rgb, deg: f64, min_sat: f64, target_light: Option<f64>) -> Rgb {
    let max = c.0.max(c.1).max(c.2);
    let min = c.0.min(c.1).min(c.2);
    let mut l = (max + min) * 0.5;
    let d = max - min;
    let (mut h, mut s) = if d < 1e-5 {
        (210.0, 0.5)
    } else {
        let s = if l > 0.5 {
            d / (2.0 - max - min)
        } else {
            d / (max + min)
        };
        let h = if (max - c.0).abs() < 1e-5 {
            ((c.1 - c.2) / d + if c.1 < c.2 { 6.0 } else { 0.0 }) * 60.0
        } else if (max - c.1).abs() < 1e-5 {
            ((c.2 - c.0) / d + 2.0) * 60.0
        } else {
            ((c.0 - c.1) / d + 4.0) * 60.0
        };
        (h, s)
    };

    h = (h + deg).rem_euclid(360.0);
    s = s.max(min_sat).clamp(0.35, 0.92);
    if let Some(tl) = target_light {
        l = (l * 0.45 + tl * 0.55).clamp(0.35, 0.82);
    }

    let c_val = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c_val * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = l - c_val * 0.5;
    let (r1, g1, b1) = match h as u32 {
        0..=59 => (c_val, x, 0.0),
        60..=119 => (x, c_val, 0.0),
        120..=179 => (0.0, c_val, x),
        180..=239 => (0.0, x, c_val),
        240..=299 => (x, 0.0, c_val),
        _ => (c_val, 0.0, x),
    };
    Rgb(
        (r1 + m).clamp(0.0, 1.0),
        (g1 + m).clamp(0.0, 1.0),
        (b1 + m).clamp(0.0, 1.0),
    )
}

/// Resolve the active `WeatherColors` from Omarchy's `colors.toml` (when in Omarchy mode)
/// or by harmonizing against the active `Palette`.
pub fn weather_colors_for(p: &Palette) -> WeatherColors {
    // Charts ask on every frame; work it out (and read colors.toml) once per palette.
    thread_local! {
        static LAST: std::cell::RefCell<Option<(String, WeatherColors)>> = const { std::cell::RefCell::new(None) };
    }
    let key = format!("{:?}{}{}{}", prefs::get().mode, p.accent, p.danger, p.light);
    if let Some(wc) = LAST.with(|l| l.borrow().as_ref().filter(|(k, _)| *k == key).map(|(_, wc)| *wc)) {
        return wc;
    }
    let wc = derive_weather_colors(p);
    LAST.with(|l| *l.borrow_mut() = Some((key, wc)));
    wc
}

fn derive_weather_colors(p: &Palette) -> WeatherColors {
    let accent = Rgb::hex(&p.accent);
    let danger = Rgb::hex(&p.danger);
    let tl = if p.light { Some(0.46) } else { Some(0.72) };

    let mut sun = danger.mix(rotate_hue(accent, 155.0, 0.72, tl), 0.65);
    let mut warm = danger.mix(sun, 0.52);
    let mut rain = rotate_hue(accent, -28.0, 0.68, tl);
    let mut wind = rotate_hue(accent, -85.0, 0.62, tl);
    let mut uv = rotate_hue(accent, 65.0, 0.68, tl);
    let mut cold = accent;

    if prefs::get().mode == ThemeMode::Omarchy
        && let Ok(text) = std::fs::read_to_string(nyx_core::paths::omarchy_colors())
        && let Ok(table) = toml::from_str::<toml::Table>(&text)
    {
        let get_rgb = |key: &str| {
            table
                .get(key)
                .and_then(|v| v.as_str())
                .map(Rgb::hex)
        };
        if let Some(c) = get_rgb("yellow").or_else(|| get_rgb("bright_yellow")) {
            sun = c;
        }
        if let Some(c) = get_rgb("orange") {
            warm = c;
        } else {
            warm = sun.mix(danger, 0.45);
        }
        if let Some(c) = get_rgb("cyan").or_else(|| get_rgb("bright_cyan")) {
            rain = c;
        }
        if let Some(c) = get_rgb("green").or_else(|| get_rgb("bright_green")) {
            wind = c;
        }
        if let Some(c) = get_rgb("magenta").or_else(|| get_rgb("bright_magenta")) {
            uv = c;
        }
        if let Some(c) = get_rgb("blue").or_else(|| get_rgb("bright_blue")) {
            cold = c;
        }
    }

    WeatherColors {
        sun,
        warm,
        rain,
        wind,
        uv,
        cold,
        danger,
    }
}

fn extra_colors(p: &Palette) -> String {
    let wc = weather_colors_for(p);
    format!(
        "@define-color weather_sun {};\n\
         @define-color weather_warm {};\n\
         @define-color weather_rain {};\n\
         @define-color weather_wind {};\n\
         @define-color weather_uv {};\n\
         @define-color weather_cold {};\n",
        wc.sun.to_hex(),
        wc.warm.to_hex(),
        wc.rain.to_hex(),
        wc.wind.to_hex(),
        wc.uv.to_hex(),
        wc.cold.to_hex(),
    )
}

/// Semantic CSS class for coloring a weather condition icon with the active theme palette.
pub fn condition_color_class(cond: WeatherCondition, is_day: bool) -> &'static str {
    match cond {
        WeatherCondition::Clear => {
            if is_day {
                "cond-sun"
            } else {
                "cond-uv"
            }
        }
        WeatherCondition::MostlyClear => {
            if is_day {
                "cond-sun"
            } else {
                "cond-cold"
            }
        }
        WeatherCondition::PartlyCloudy => {
            if is_day {
                "cond-warm"
            } else {
                "cond-cold"
            }
        }
        WeatherCondition::Overcast | WeatherCondition::Fog => "cond-cloud",
        WeatherCondition::Drizzle
        | WeatherCondition::Rain
        | WeatherCondition::HeavyRain
        | WeatherCondition::RainShowers => "cond-rain",
        WeatherCondition::FreezingDrizzle
        | WeatherCondition::FreezingRain
        | WeatherCondition::Snow
        | WeatherCondition::HeavySnow
        | WeatherCondition::SnowGrains
        | WeatherCondition::SnowShowers => "cond-cold",
        WeatherCondition::Thunderstorm | WeatherCondition::ThunderstormHail => "cond-uv",
        WeatherCondition::Unknown => "cond-cloud",
    }
}

/// Install the stylesheet and the current palette, and follow Omarchy live.
pub fn install() {
    nyx_theme::install(Style {
        window_class: "weather-window",
        base_skip: &[],
        css: include_str!("style.css"),
        no_glow_css: "",
        extra_colors: Some(extra_colors),
        prefs: || {
            let p = prefs::get();
            ThemePrefs {
                mode: p.mode,
                theme: p.theme,
                glow: p.glow,
                reduce_motion: p.reduce_motion,
            }
        },
        set_theme: Some(|mode, theme| {
            prefs::update(|p| {
                p.mode = mode;
                p.theme = theme;
            })
        }),
    });
}
