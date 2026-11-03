use chrono::{Datelike, Local};
use gtk4 as gtk;
use gtk4::prelude::*;

use super::{Ctx, Size, Widget, card, clear, label, open_url};
use crate::shared::Shared;
use crate::sources::weather::{self, describe};

pub struct Weather {
    data: Shared<weather::Weather>,
}

pub fn new(ctx: &Ctx) -> Option<Box<dyn Widget>> {
    let data = Shared::new();
    let (tx, rx) = async_channel::unbounded();
    weather::watch(ctx.cfg.weather.clone(), tx);
    data.feed(rx);
    Some(Box::new(Weather { data }))
}

impl Widget for Weather {
    fn title(&self) -> &'static str {
        "Weather"
    }
    fn icon(&self) -> &'static str {
        "weather-few-clouds-symbolic"
    }
    fn sizes(&self) -> &'static [Size] {
        &[Size::Small, Size::Mini, Size::Medium, Size::Large]
    }
    fn build(&self, ctx: &Ctx, size: Size) -> gtk::Widget {
        build(ctx, &self.data, size)
    }
    fn summary(&self, _: &Ctx, l: &gtk::Label) {
        self.data.bind(l, |l, w| {
            l.set_text(&if w.error.is_some() { "—".into() } else { format!("{:.0}° {}", w.temp, describe(w.code, w.is_day).1) })
        });
    }
}

fn big(text: &str, class: &str) -> gtk::Label {
    let l = label(text, class);
    l.set_ellipsize(gtk4::pango::EllipsizeMode::None);
    l.set_max_width_chars(-1);
    l
}

fn icon(px: i32) -> gtk::Image {
    let i = gtk::Image::new();
    i.set_pixel_size(px);
    i
}

fn build(ctx: &Ctx, data: &Shared<weather::Weather>, size: Size) -> gtk::Widget {
    let c = card("weather", size);
    let loc = ctx.cfg.weather.location.clone();
    super::on_click(&c, move || open_url(&format!("https://www.windy.com/?{}", loc.replace(' ', "+"))));

    // the current conditions, laid out for the size
    let place = label("", "weather-place");
    let temp = big("", "weather-temp");
    let now_icon = icon(22);
    now_icon.set_halign(gtk::Align::Start);
    let cond = label("", "weather-cond");
    let range = label("", "weather-range dim");
    let hours = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    hours.set_homogeneous(true);
    hours.add_css_class("weather-hours");
    let days = gtk::Box::new(gtk::Orientation::Vertical, 2);
    days.add_css_class("weather-days");

    match size {
        Size::Mini => {
            now_icon.set_pixel_size(20);
            now_icon.set_halign(gtk::Align::Center);
            temp.add_css_class("mini");
            temp.set_xalign(0.5);
            temp.set_vexpand(true);
            c.append(&now_icon);
            c.append(&temp);
        }
        Size::Small => {
            let spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
            spacer.set_vexpand(true);
            for w in [place.upcast_ref::<gtk::Widget>(), temp.upcast_ref(), spacer.upcast_ref(), now_icon.upcast_ref(), cond.upcast_ref(), range.upcast_ref()] {
                c.append(w);
            }
        }
        Size::Medium | Size::Large => {
            let top = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            let left = gtk::Box::new(gtk::Orientation::Vertical, 0);
            left.set_hexpand(true);
            left.append(&place);
            left.append(&temp);
            let right = gtk::Box::new(gtk::Orientation::Vertical, 2);
            right.set_valign(gtk::Align::Start);
            now_icon.set_halign(gtk::Align::End);
            for l in [&cond, &range] {
                l.set_xalign(1.0);
                l.set_ellipsize(gtk4::pango::EllipsizeMode::None);
                l.set_max_width_chars(-1);
            }
            right.append(&now_icon);
            right.append(&cond);
            right.append(&range);
            top.append(&left);
            top.append(&right);
            c.append(&top);
            hours.set_vexpand(size == Size::Medium);
            hours.set_valign(gtk::Align::End);
            c.append(&hours);
            if size == Size::Large {
                let sep = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                sep.add_css_class("weather-sep");
                c.append(&sep);
                days.set_vexpand(true);
                c.append(&days);
            }
        }
    }

    data.bind(&c, move |c, w| {
        if let Some(e) = &w.error {
            place.set_text("Weather");
            temp.set_text("—");
            cond.set_text(e);
            range.set_text("");
            return;
        }
        c.remove_css_class("night");
        if !w.is_day {
            c.add_css_class("night");
        }
        let (name, text) = describe(w.code, w.is_day);
        place.set_text(&w.place);
        temp.set_text(&format!("{:.0}°", w.temp));
        now_icon.set_icon_name(Some(name));
        cond.set_text(text);
        range.set_text(&format!("H:{:.0}°  L:{:.0}°", w.high, w.low));

        clear(&hours);
        for (i, h) in w.hours.iter().take(6).enumerate() {
            let col = gtk::Box::new(gtk::Orientation::Vertical, 4);
            let t = big(if i == 0 { "Now" } else { &h.hour }, "tiny dim");
            t.set_xalign(0.5);
            let ic = icon(18);
            ic.set_icon_name(Some(describe(h.code, h.is_day).0));
            let v = big(&format!("{:.0}°", h.temp), "weather-hour");
            v.set_xalign(0.5);
            col.append(&t);
            col.append(&ic);
            col.append(&v);
            hours.append(&col);
        }

        clear(&days);
        let today = Local::now().date_naive();
        let (lo, hi) = w.days.iter().fold((f64::MAX, f64::MIN), |(lo, hi), d| (lo.min(d.low), hi.max(d.high)));
        for d in w.days.iter().take(5) {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            row.add_css_class("weather-day");
            let name = label(&if d.date == today { "Today".into() } else { d.date.weekday().to_string() }, "weather-day-name");
            name.set_size_request(46, -1);
            let ic = icon(16);
            ic.set_icon_name(Some(describe(d.code, true).0));
            let low = big(&format!("{:.0}°", d.low), "small dim");
            low.set_size_request(28, -1);
            low.set_xalign(1.0);
            // a bar for the day's range on the week's scale, like the Weather app
            let track = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            track.add_css_class("weather-track");
            track.set_hexpand(true);
            track.set_valign(gtk::Align::Center);
            let span = (hi - lo).max(1.0);
            let lead = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            let fill = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            fill.add_css_class("weather-fill");
            let width = 110.0;
            lead.set_size_request(((d.low - lo) / span * width) as i32, -1);
            fill.set_size_request((((d.high - d.low) / span) * width).max(6.0) as i32, -1);
            track.append(&lead);
            track.append(&fill);
            let high = big(&format!("{:.0}°", d.high), "small");
            high.set_size_request(28, -1);
            high.set_xalign(1.0);
            row.append(&name);
            row.append(&ic);
            row.append(&low);
            row.append(&track);
            row.append(&high);
            days.append(&row);
        }
    });
    c.upcast()
}
