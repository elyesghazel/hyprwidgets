use gtk4 as gtk;
use gtk4::prelude::*;

use super::{Hub, Size, card, label};

pub fn build(hub: &Hub) -> gtk::Widget {
    let c = card("clock", Size::Small);
    let day = label("", "eyebrow accent");
    let time = label("", "clock-time");
    time.set_ellipsize(gtk4::pango::EllipsizeMode::None);
    time.set_max_width_chars(-1);
    time.set_halign(gtk::Align::Start);
    if hub.cfg.clock.seconds {
        time.add_css_class("seconds");
    }
    let date = label("", "clock-date dim");
    let spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    spacer.set_vexpand(true);
    c.append(&day);
    c.append(&spacer);
    c.append(&time);
    c.append(&date);

    let cfg = hub.cfg.clock.clone();
    hub.now.bind(&c, move |_, now| {
        day.set_text(&now.format("%A").to_string().to_uppercase());
        let t = match (cfg.twelve_hour, cfg.seconds) {
            (true, true) => now.format("%-I:%M:%S").to_string(),
            (true, false) => now.format("%-I:%M").to_string(),
            (false, true) => now.format("%H:%M:%S").to_string(),
            (false, false) => now.format("%H:%M").to_string(),
        };
        time.set_text(&t);
        let suffix = if cfg.twelve_hour { now.format(" · %p").to_string() } else { String::new() };
        date.set_text(&format!("{}{suffix} · W{}", now.format("%-d %B"), now.format("%V").to_string().trim_start_matches('0')));
    });
    c.upcast()
}
