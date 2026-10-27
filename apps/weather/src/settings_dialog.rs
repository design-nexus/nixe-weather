//! Settings as a card over the window: the groups listed on the left, the
//! chosen group's options on the right, and a search across all of them.

use crate::sections::settings;
use crate::{widgets, window};
use gtk::prelude::*;
use nyx_ui::settings_card::{self, Spec};

pub use nyx_ui::settings_card::{escape, fit, is_open};

const GROUPS: &[(&str, &str, &str)] = &[
    ("Units & forecast", "weather-clear-symbolic", "Temperature, wind, refresh, API"),
    ("Locations", "mark-location-symbolic", "Saved places and active city"),
    ("Weather window", "applications-graphics-symbolic", "Theme, glow, motion"),
    ("Keyboard", "input-keyboard-symbolic", "Shortcuts"),
];

const SPEC: Spec = Spec {
    groups: GROUPS,
    search_text: None,
};

pub fn open() {
    let (Some(overlay), Some(window)) = (window::overlay(), window::window()) else {
        return;
    };
    let page = widgets::page("settings");
    settings::build(&page);
    settings_card::open_page(&overlay, &window, SPEC, page.root.upcast_ref(), &page.body);
}
