use chrono::{Datelike, Local};
use gtk4 as gtk;
use gtk4::prelude::*;

use super::{Hub, Size, card, clear, label, open_url, time};
use crate::sources::calendar::Event;

/// Left: today's weekday and date. Right: the next events, grouped by day.
pub fn build(hub: &Hub) -> gtk::Widget {
    let c = card("calendar", Size::Wide);
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    row.set_vexpand(true);
    c.append(&row);

    let today = gtk::Box::new(gtk::Orientation::Vertical, 0);
    today.add_css_class("cal-today");
    let weekday = label("", "eyebrow red");
    let day = label("", "cal-day");
    day.set_ellipsize(gtk4::pango::EllipsizeMode::None);
    day.set_max_width_chars(-1);
    let summary = label("", "small dim");
    summary.set_wrap(true);
    summary.set_ellipsize(gtk4::pango::EllipsizeMode::None);
    summary.set_max_width_chars(10);
    let spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    spacer.set_vexpand(true);
    today.append(&weekday);
    today.append(&day);
    today.append(&spacer);
    today.append(&summary);
    row.append(&today);

    let list = gtk::Box::new(gtk::Orientation::Vertical, 6);
    list.set_hexpand(true);
    list.add_css_class("cal-list");
    row.append(&list);

    let open = hub.cfg.calendar.open.clone();
    let open_day = move |d: chrono::NaiveDate| {
        open_url(&open.replace("{y}", &d.year().to_string()).replace("{m}", &d.month().to_string()).replace("{d}", &d.day().to_string()))
    };
    {
        let open_day = open_day.clone();
        super::on_click(&today, move || open_day(Local::now().date_naive()));
    }

    hub.now.bind(&c, move |_, now| {
        weekday.set_text(&now.format("%A").to_string().to_uppercase());
        day.set_text(&now.day().to_string());
    });

    let twelve = hub.cfg.clock.twelve_hour;
    let max = hub.cfg.calendar.max_events;
    hub.agenda.bind(&c, move |_, agenda| {
        clear(&list);
        let now = Local::now();
        let today_left = agenda.events.iter().filter(|e| e.start.date_naive() <= now.date_naive()).count();
        summary.set_text(&match (today_left, &agenda.error) {
            (_, Some(e)) if agenda.events.is_empty() => e.clone(),
            (0, _) => "No more events today".into(),
            (1, _) => "1 event today".into(),
            (n, _) => format!("{n} events today"),
        });
        if agenda.events.is_empty() {
            let empty = label("Nothing coming up", "dim");
            empty.set_vexpand(true);
            empty.set_xalign(0.5);
            list.append(&empty);
            return;
        }
        let mut last_day = None;
        for e in agenda.events.iter().take(max) {
            let d = e.start.date_naive().max(now.date_naive());
            if last_day != Some(d) {
                let heading = match (d - now.date_naive()).num_days() {
                    0 => "TODAY".to_string(),
                    1 => "TOMORROW".to_string(),
                    _ => e.start.format("%A %-d").to_string().to_uppercase(),
                };
                let h = label(&heading, "cal-heading dim");
                if last_day.is_some() {
                    h.add_css_class("gap");
                }
                list.append(&h);
                last_day = Some(d);
            }
            let item = event_row(e, twelve, &now);
            let open_day = open_day.clone();
            super::on_click(&item, move || open_day(d));
            list.append(&item);
        }
    });
    c.upcast()
}

fn event_row(e: &Event, twelve: bool, now: &chrono::DateTime<Local>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.add_css_class("cal-event");
    row.set_tooltip_text(Some(&e.calendar));
    let bar = gtk::Box::new(gtk::Orientation::Vertical, 0);
    bar.add_css_class("cal-bar");
    bar.set_size_request(3, -1);
    let provider = gtk::CssProvider::new();
    provider.load_from_string(&format!("box {{ background: {}; }}", e.color));
    #[allow(deprecated)]
    bar.style_context().add_provider(&provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    row.append(&bar);

    let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
    text.set_hexpand(true);
    text.append(&label(&e.title, "cal-title"));
    let when = if e.all_day {
        "All day".to_string()
    } else if e.start <= *now {
        format!("Now · until {}", time(&e.end, twelve))
    } else {
        format!("{} – {}", time(&e.start, twelve), time(&e.end, twelve))
    };
    let sub = if e.location.is_empty() { when } else { format!("{when} · {}", e.location) };
    text.append(&label(&sub, "small dim"));
    row.append(&text);
    row
}
