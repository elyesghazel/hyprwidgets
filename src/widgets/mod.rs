//! The cards. Each `build` makes one card for one monitor and binds it to the shared data.

pub mod calendar;
pub mod clock;
pub mod media;
pub mod reminders;
pub mod weather;

use chrono::{DateTime, Local};
use gtk4 as gtk;
use gtk4::prelude::*;

use crate::config::Config;
use crate::shared::Shared;
use crate::sources;

/// Everything the cards read from, created once and shared by every monitor.
pub struct Hub {
    pub cfg: Config,
    pub now: Shared<DateTime<Local>>,
    pub weather: Shared<sources::weather::Weather>,
    pub agenda: Shared<sources::calendar::Agenda>,
    pub reminders: Shared<sources::reminders::State>,
    pub reminders_cmd: Option<async_channel::Sender<sources::reminders::Cmd>>,
    pub media: Shared<sources::media::Track>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Size {
    /// A square, two fit side by side.
    Small,
    /// Full column width.
    Wide,
}

pub fn size(name: &str) -> Size {
    match name {
        "clock" | "weather" => Size::Small,
        _ => Size::Wide,
    }
}

pub fn build(name: &str, hub: &Hub) -> Option<gtk::Widget> {
    Some(match name {
        "clock" => clock::build(hub),
        "weather" => weather::build(hub),
        "calendar" => calendar::build(hub),
        "reminders" if hub.reminders_cmd.is_some() => reminders::build(hub),
        "media" => media::build(hub),
        _ => return None,
    })
}

/// An empty card of the given size with its CSS class.
pub fn card(class: &str, size: Size) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 0);
    b.add_css_class("card");
    b.add_css_class(class);
    b.add_css_class(if size == Size::Small { "small" } else { "wide" });
    b.set_overflow(gtk::Overflow::Hidden);
    b
}

pub fn label(text: &str, class: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    for c in class.split_whitespace() {
        l.add_css_class(c);
    }
    l.set_xalign(0.0);
    l.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    // ellipsized labels would otherwise ask for their whole text and widen the surface
    l.set_max_width_chars(1);
    l
}

pub fn open_url(url: &str) {
    let _ = std::process::Command::new("xdg-open")
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// Run `f` on a plain click (not a drag), so cards stay clickable without being buttons.
pub fn on_click(w: &impl IsA<gtk::Widget>, f: impl Fn() + 'static) {
    let click = gtk::GestureClick::new();
    click.connect_released(move |g, _, _, _| {
        g.set_state(gtk::EventSequenceState::Claimed);
        f();
    });
    w.as_ref().add_controller(click);
    w.as_ref().set_cursor_from_name(Some("pointer"));
}

pub fn clear(b: &gtk::Box) {
    while let Some(c) = b.first_child() {
        b.remove(&c);
    }
}

/// "14:05" or "2:05 PM".
pub fn time(t: &DateTime<Local>, twelve: bool) -> String {
    if twelve { t.format("%-I:%M %p").to_string() } else { t.format("%H:%M").to_string() }
}
