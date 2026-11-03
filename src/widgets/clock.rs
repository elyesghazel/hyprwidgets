use chrono::{DateTime, Local};
use gtk4 as gtk;
use gtk4::prelude::*;

use super::{Ctx, Size, Widget, card, label};
use crate::config;

pub struct Clock;

pub fn new(_: &Ctx) -> Option<Box<dyn Widget>> {
    Some(Box::new(Clock))
}

impl Widget for Clock {
    fn title(&self) -> &'static str {
        "Clock"
    }
    fn icon(&self) -> &'static str {
        "preferences-system-time-symbolic"
    }
    fn sizes(&self) -> &'static [Size] {
        &[Size::Small, Size::Mini, Size::Medium]
    }
    fn build(&self, ctx: &Ctx, size: Size) -> gtk::Widget {
        build(ctx, size)
    }
    fn summary(&self, ctx: &Ctx, l: &gtk::Label) {
        let twelve = ctx.cfg.clock.twelve_hour;
        ctx.now.bind(l, move |l, now| l.set_text(&super::time(now, twelve)));
    }
}

fn big(class: &str) -> gtk::Label {
    let l = label("", class);
    l.set_ellipsize(gtk4::pango::EllipsizeMode::None);
    l.set_max_width_chars(-1);
    l.set_halign(gtk::Align::Start);
    l
}

fn clock_text(cfg: &config::Clock, now: &DateTime<Local>) -> String {
    match (cfg.twelve_hour, cfg.seconds) {
        (true, true) => now.format("%-I:%M:%S").to_string(),
        (true, false) => now.format("%-I:%M").to_string(),
        (false, true) => now.format("%H:%M:%S").to_string(),
        (false, false) => now.format("%H:%M").to_string(),
    }
}

fn week(now: &DateTime<Local>) -> String {
    now.format("%V").to_string().trim_start_matches('0').to_string()
}

fn build(ctx: &Ctx, size: Size) -> gtk::Widget {
    let c = card("clock", size);
    let cfg = ctx.cfg.clock.clone();
    match size {
        // the time and the weekday, nothing else
        Size::Mini => {
            let day = label("", "eyebrow accent");
            day.set_xalign(0.5);
            let time = big("clock-time mini");
            time.set_halign(gtk::Align::Center);
            time.set_vexpand(true);
            c.append(&day);
            c.append(&time);
            ctx.now.bind(&c, move |_, now| {
                day.set_text(&now.format("%a").to_string().to_uppercase());
                time.set_text(&super::time(now, cfg.twelve_hour).replace(" AM", "").replace(" PM", ""));
            });
        }
        // a big time on the left, the date stacked on the right
        Size::Medium | Size::Large => {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 16);
            row.set_vexpand(true);
            let time = big("clock-time large");
            time.set_valign(gtk::Align::Center);
            time.set_hexpand(true);
            let side = gtk::Box::new(gtk::Orientation::Vertical, 2);
            side.set_valign(gtk::Align::Center);
            let day = label("", "eyebrow accent");
            let date = label("", "clock-date");
            let wk = label("", "clock-date dim");
            for l in [&day, &date, &wk] {
                l.set_xalign(1.0);
                l.set_ellipsize(gtk4::pango::EllipsizeMode::None);
                l.set_max_width_chars(-1);
                side.append(l);
            }
            row.append(&time);
            row.append(&side);
            c.append(&row);
            ctx.now.bind(&c, move |_, now| {
                time.set_text(&clock_text(&cfg, now));
                day.set_text(&now.format("%A").to_string().to_uppercase());
                date.set_text(&now.format("%-d %B").to_string());
                let ampm = if cfg.twelve_hour { now.format(" · %p").to_string() } else { String::new() };
                wk.set_text(&format!("Week {}{ampm}", week(now)));
            });
        }
        Size::Small => {
            let day = label("", "eyebrow accent");
            let time = big("clock-time");
            if cfg.seconds {
                time.add_css_class("seconds");
            }
            let date = label("", "clock-date dim");
            let spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
            spacer.set_vexpand(true);
            c.append(&day);
            c.append(&spacer);
            c.append(&time);
            c.append(&date);
            ctx.now.bind(&c, move |_, now| {
                day.set_text(&now.format("%A").to_string().to_uppercase());
                time.set_text(&clock_text(&cfg, now));
                let suffix = if cfg.twelve_hour { now.format(" · %p").to_string() } else { String::new() };
                date.set_text(&format!("{}{suffix} · W{}", now.format("%-d %B"), week(now)));
            });
        }
    }
    c.upcast()
}
