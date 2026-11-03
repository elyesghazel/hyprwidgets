use std::cell::RefCell;
use std::rc::Rc;

use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{gdk, gio};

use super::{Hub, Size, card, label};
use crate::sources::media::{Track, control};

pub fn build(hub: &Hub) -> gtk::Widget {
    let c = card("media", Size::Wide);
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    c.append(&row);

    let art = gtk::Picture::new();
    art.set_content_fit(gtk::ContentFit::Cover);
    art.set_size_request(76, 76);
    art.set_can_shrink(true);
    let art_frame = gtk::Overlay::new();
    art_frame.add_css_class("media-art");
    art_frame.set_overflow(gtk::Overflow::Hidden);
    art_frame.set_size_request(76, 76);
    art_frame.set_valign(gtk::Align::Center);
    let fallback = gtk::Image::from_icon_name("audio-x-generic-symbolic");
    fallback.set_pixel_size(28);
    art_frame.set_child(Some(&fallback));
    art_frame.add_overlay(&art);
    row.append(&art_frame);

    let info = gtk::Box::new(gtk::Orientation::Vertical, 0);
    info.set_hexpand(true);
    info.set_valign(gtk::Align::Center);
    let player = label("", "eyebrow dim");
    let title = label("Not playing", "media-title");
    let artist = label("", "dim");
    info.append(&player);
    info.append(&title);
    info.append(&artist);
    row.append(&info);

    let progress = gtk::ProgressBar::new();
    progress.add_css_class("media-progress");
    let times = gtk::CenterBox::new();
    let elapsed = label("", "tiny dim");
    let remaining = label("", "tiny dim");
    times.set_start_widget(Some(&elapsed));
    times.set_end_widget(Some(&remaining));

    let controls = gtk::Box::new(gtk::Orientation::Horizontal, 18);
    controls.set_halign(gtk::Align::Center);
    controls.add_css_class("media-controls");
    let current: Rc<RefCell<Track>> = Rc::default();
    let button = |icon: &str, action: &'static str| {
        let b = gtk::Button::from_icon_name(icon);
        b.add_css_class("flat");
        b.add_css_class("media-btn");
        let current = current.clone();
        b.connect_clicked(move |_| control(&current.borrow().player, action));
        controls.append(&b);
        b
    };
    button("media-skip-backward-symbolic", "previous");
    let play = button("media-playback-start-symbolic", "play-pause");
    play.add_css_class("media-play");
    button("media-skip-forward-symbolic", "next");

    c.append(&progress);
    c.append(&times);
    c.append(&controls);

    let hide_idle = hub.cfg.media.hide_when_idle;
    // no player at all means no update ever arrives, so start hidden
    c.set_visible(!hide_idle);
    let last_art: Rc<RefCell<String>> = Rc::default();
    {
        let current = current.clone();
        let progress = progress.clone();
        let (elapsed, remaining) = (elapsed.clone(), remaining.clone());
        hub.media.bind(&c, move |c, t| {
            *current.borrow_mut() = t.clone();
            c.set_visible(t.active() || !hide_idle);
            player.set_text(&pretty_player(&t.player).to_uppercase());
            title.set_text(if t.active() { &t.title } else { "Not playing" });
            artist.set_text(&t.artist);
            play.set_icon_name(if t.playing { "media-playback-pause-symbolic" } else { "media-playback-start-symbolic" });
            if *last_art.borrow() != t.art {
                *last_art.borrow_mut() = t.art.clone();
                set_art(&art, &t.art);
            }
            tick(t, &progress, &elapsed, &remaining);
        });
    }
    hub.now.bind(&c, move |_, _| tick(&current.borrow(), &progress, &elapsed, &remaining));
    c.upcast()
}

fn tick(t: &Track, progress: &gtk::ProgressBar, elapsed: &gtk::Label, remaining: &gtk::Label) {
    let visible = t.length > 0.0;
    progress.set_visible(visible);
    elapsed.set_visible(visible);
    remaining.set_visible(visible);
    if visible {
        let pos = t.position_now();
        progress.set_fraction((pos / t.length).clamp(0.0, 1.0));
        elapsed.set_text(&clock(pos));
        remaining.set_text(&format!("-{}", clock(t.length - pos)));
    }
}

fn clock(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

fn pretty_player(name: &str) -> String {
    let base = name.split('.').next().unwrap_or(name);
    match base {
        "spotify" => "Spotify".into(),
        "firefox" => "Firefox".into(),
        "chromium" | "chrome" => "Chrome".into(),
        "zen" => "Zen".into(),
        "mpv" => "mpv".into(),
        other => {
            let mut c = other.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        }
    }
}

/// Album art from a file:// path or a web address (Spotify serves https). Downloads off the
/// main thread into the cache.
fn set_art(pic: &gtk::Picture, url: &str) {
    pic.set_paintable(None::<&gdk::Paintable>);
    if let Some(path) = url.strip_prefix("file://") {
        pic.set_file(Some(&gio::File::for_path(path)));
        return;
    }
    if !url.starts_with("http") {
        return;
    }
    let name: String = url.chars().rev().take(40).filter(|c| c.is_alphanumeric()).collect();
    let path = crate::config::cache_dir().join(format!("art-{name}"));
    if path.exists() {
        pic.set_file(Some(&gio::File::for_path(&path)));
        return;
    }
    let (tx, rx) = async_channel::bounded::<bool>(1);
    let url = url.to_string();
    let target = path.clone();
    std::thread::spawn(move || {
        let ok = ureq::get(&url)
            .call()
            .ok()
            .and_then(|mut r| r.body_mut().read_to_vec().ok())
            .is_some_and(|bytes| std::fs::write(&target, bytes).is_ok());
        let _ = tx.send_blocking(ok);
    });
    let weak = pic.downgrade();
    gtk4::glib::spawn_future_local(async move {
        if let (Ok(true), Some(pic)) = (rx.recv().await, weak.upgrade()) {
            pic.set_file(Some(&gio::File::for_path(&path)));
        }
    });
}
