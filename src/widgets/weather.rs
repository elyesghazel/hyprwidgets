use gtk4 as gtk;
use gtk4::prelude::*;

use super::{Hub, Size, card, label, open_url};
use crate::sources::weather::describe;

pub fn build(hub: &Hub) -> gtk::Widget {
    let c = card("weather", Size::Small);
    let place = label("", "weather-place");
    let temp = label("", "weather-temp");
    temp.set_ellipsize(gtk4::pango::EllipsizeMode::None);
    temp.set_max_width_chars(-1);
    let icon = gtk::Image::new();
    icon.set_pixel_size(22);
    icon.set_halign(gtk::Align::Start);
    let cond = label("", "weather-cond");
    let range = label("", "weather-range dim");
    let spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    spacer.set_vexpand(true);
    for w in [place.upcast_ref::<gtk::Widget>(), temp.upcast_ref(), spacer.upcast_ref(), icon.upcast_ref(), cond.upcast_ref(), range.upcast_ref()] {
        c.append(w);
    }

    let loc = hub.cfg.weather.location.clone();
    super::on_click(&c, move || open_url(&format!("https://www.windy.com/?{}", loc.replace(' ', "+"))));

    hub.weather.bind(&c, move |c, w| {
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
        icon.set_icon_name(Some(name));
        cond.set_text(text);
        range.set_text(&format!("H:{:.0}°  L:{:.0}°", w.high, w.low));
    });
    c.upcast()
}
