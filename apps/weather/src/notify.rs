//! Desktop notifications for new severe-weather alerts.

use crate::prefs;
use gtk::gio;
use gtk::prelude::*;
use nixe_weather_core::{AlertSeverity, ForecastSnapshot};

/// Ids remembered, so an alert is notified once even after it drops out and returns.
const REMEMBERED: usize = 100;

/// Notify each Severe or Extreme alert in `snap` not notified before.
pub fn new_alerts(snap: &ForecastSnapshot) {
    let p = prefs::get();
    if !p.notify_alerts {
        return;
    }
    let fresh: Vec<_> = snap
        .alerts
        .iter()
        .filter(|a| a.severity >= AlertSeverity::Severe && !p.notified_alerts.contains(&a.id))
        .collect();
    if fresh.is_empty() {
        return;
    }
    if let Some(app) = gio::Application::default() {
        for a in &fresh {
            let n = gio::Notification::new(&format!("{} · {}", a.event, snap.location.name));
            n.set_body(Some(&a.headline));
            n.set_priority(if a.severity == AlertSeverity::Extreme {
                gio::NotificationPriority::Urgent
            } else {
                gio::NotificationPriority::High
            });
            app.send_notification(Some(&a.id), &n);
        }
    }
    prefs::update(|p| {
        p.notified_alerts.extend(fresh.iter().map(|a| a.id.clone()));
        let extra = p.notified_alerts.len().saturating_sub(REMEMBERED);
        p.notified_alerts.drain(..extra);
    });
}
