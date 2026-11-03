//! `hyprwidgets settings`: the config in a window. Every change goes straight into config.toml
//! (comments kept) and the running widgets pick it up from there.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};
use toml_edit::{Array, ArrayOfTables, DocumentMut, Item, Table, Value};

use crate::config::{self, Config};
use crate::setup;
use crate::theme::{self, ACCENTS, PRESETS};

const CSS: &str = include_str!("settings.css");
const README: &str = "https://github.com/elyesghazel/hyprwidgets#setup-in-detail";

/// config.toml as an editable document.
struct Store {
    doc: RefCell<DocumentMut>,
    /// The file didn't parse: nothing is written so it isn't lost.
    broken: Option<String>,
    timers: RefCell<HashMap<String, glib::SourceId>>,
}

impl Store {
    fn load() -> Rc<Store> {
        let text = std::fs::read_to_string(config::path()).unwrap_or_else(|_| config::DEFAULT.to_owned());
        let (doc, broken) = match text.parse::<DocumentMut>() {
            Ok(d) => match config::try_load() {
                Ok(_) => (d, None),
                Err(e) => (d, Some(e)),
            },
            Err(e) => (config::DEFAULT.parse().expect("default config"), Some(e.to_string())),
        };
        Rc::new(Store { doc: RefCell::new(doc), broken, timers: RefCell::default() })
    }

    fn save(&self) {
        if self.broken.is_some() {
            return;
        }
        let _ = std::fs::create_dir_all(config::dir());
        if let Err(e) = std::fs::write(config::path(), self.doc.borrow().to_string()) {
            eprintln!("hyprwidgets: {}: {e}", config::path().display());
        }
    }

    /// Sets `key` in `[table]` (None: the top), keeping the comment next to it.
    fn set(&self, table: Option<&str>, key: &str, v: impl Into<Value>) {
        {
            let mut doc = self.doc.borrow_mut();
            let t = match table {
                None => doc.as_table_mut(),
                Some(name) => match doc.as_table_mut().entry(name).or_insert(toml_edit::table()).as_table_mut() {
                    Some(t) => t,
                    None => return,
                },
            };
            let mut v: Value = v.into();
            if let Some(old) = t.get(key).and_then(Item::as_value) {
                *v.decor_mut() = old.decor().clone();
            }
            t.insert(key, Item::Value(v));
        }
        self.save();
    }

    /// Runs `f` once typing has paused, so the widgets don't restart on every key.
    fn later(self: &Rc<Self>, id: &str, f: impl FnOnce(&Store) + 'static) {
        if let Some(t) = self.timers.borrow_mut().remove(id) {
            t.remove();
        }
        let (me, id2) = (self.clone(), id.to_owned());
        let t = glib::timeout_add_local_once(Duration::from_millis(700), move || {
            me.timers.borrow_mut().remove(&id2);
            f(&me);
        });
        self.timers.borrow_mut().insert(id.to_owned(), t);
    }
}

/// `page`: look, widgets, calendars or setup.
pub fn run(page: Option<String>) -> glib::ExitCode {
    let app = gtk::Application::builder().application_id(format!("{}.settings", crate::APP_ID)).build();
    app.connect_activate(move |app| match app.active_window() {
        Some(w) => w.present(),
        None => build(app, page.as_deref()),
    });
    app.run_with_args::<&str>(&[])
}

fn build(app: &gtk::Application, page: Option<&str>) {
    let display = gdk::Display::default().expect("no display");
    let css = gtk::CssProvider::new();
    css.load_from_string(CSS);
    // above ~/.config/gtk-4.0/gtk.css, which some themes use to restyle everything
    gtk::style_context_add_provider_for_display(&display, &css, gtk::STYLE_PROVIDER_PRIORITY_USER + 1);

    let store = Store::load();
    let cfg = config::load();

    let win = gtk::ApplicationWindow::builder().application(app).title("hyprwidgets settings").default_width(920).default_height(660).build();
    win.add_css_class("hw-settings");

    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_hexpand(true);
    stack.set_vexpand(true);
    let pages: [(&str, &str, gtk::Widget); 4] = [
        ("look", "Look", look_page(&store, &cfg)),
        ("widgets", "Widgets", widgets_page(&store, &cfg)),
        ("calendars", "Calendars", calendars_page(&store, &cfg)),
        ("setup", "Setup", setup_page(&store, &cfg)),
    ];
    for (name, title, page) in pages {
        let scroll = gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).child(&page).build();
        stack.add_titled(&scroll, Some(name), title);
    }
    // first run, or not set up yet: open on Setup
    if let Some(p) = page.filter(|p| stack.child_by_name(p).is_some()) {
        stack.set_visible_child_name(p);
    } else if !config::path().exists() || !setup::running() {
        stack.set_visible_child_name("setup");
    }

    let side = gtk::Box::new(gtk::Orientation::Vertical, 0);
    side.add_css_class("side");
    let brand = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    brand.add_css_class("brand");
    // from the binary: the icon theme may not know it yet
    let logo = gdk::Texture::from_bytes(&glib::Bytes::from_static(setup::ICON.as_bytes()))
        .map(|t| gtk::Image::from_paintable(Some(&t)))
        .unwrap_or_else(|_| gtk::Image::from_icon_name("hyprwidgets"));
    logo.set_pixel_size(28);
    brand.append(&logo);
    let name = gtk::Label::new(Some("hyprwidgets"));
    name.add_css_class("brand-name");
    brand.append(&name);
    side.append(&brand);
    let sidebar = gtk::StackSidebar::new();
    sidebar.set_stack(&stack);
    sidebar.set_vexpand(true);
    side.append(&sidebar);
    let version = gtk::Label::new(Some(concat!("v", env!("CARGO_PKG_VERSION"))));
    version.add_css_class("dim");
    version.set_margin_bottom(12);
    side.append(&version);

    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    if let Some(e) = &store.broken {
        let bar = gtk::Label::new(Some(&format!("config.toml has an error, so nothing here is saved until it's fixed:\n{e}")));
        bar.add_css_class("error-bar");
        bar.set_wrap(true);
        bar.set_xalign(0.0);
        body.append(&bar);
    }
    body.append(&stack);

    let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    root.append(&side);
    root.append(&gtk::Separator::new(gtk::Orientation::Vertical));
    root.append(&body);
    win.set_child(Some(&root));
    win.present();
}

// ── building blocks ──────────────────────────────────────

fn page_box() -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 10);
    b.add_css_class("page");
    b
}

fn heading(text: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.add_css_class("heading");
    l.set_xalign(0.0);
    l
}

fn note(text: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.add_css_class("dim");
    l.set_wrap(true);
    l.set_xalign(0.0);
    l
}

fn group() -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 0);
    b.add_css_class("group");
    b
}

/// A row with a title, an optional line under it, and a control on the right.
fn row(title: &str, subtitle: &str, control: &impl IsA<gtk::Widget>) -> (gtk::Box, gtk::Label) {
    let r = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    r.add_css_class("row");
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);
    let t = gtk::Label::new(Some(title));
    t.set_xalign(0.0);
    t.add_css_class("row-title");
    text.append(&t);
    let s = gtk::Label::new(Some(subtitle));
    s.set_xalign(0.0);
    s.set_wrap(true);
    s.add_css_class("dim");
    s.add_css_class("row-sub");
    s.set_visible(!subtitle.is_empty());
    text.append(&s);
    r.append(&text);
    control.as_ref().set_valign(gtk::Align::Center);
    r.append(control);
    (r, s)
}

fn switch(on: bool, f: impl Fn(bool) + 'static) -> gtk::Switch {
    let s = gtk::Switch::new();
    s.set_active(on);
    s.connect_active_notify(move |s| f(s.is_active()));
    s
}

fn dropdown(options: &[&str], selected: usize, f: impl Fn(usize) + 'static) -> gtk::DropDown {
    let d = gtk::DropDown::from_strings(options);
    d.set_selected(selected as u32);
    d.connect_selected_notify(move |d| f(d.selected() as usize));
    d
}

// ── Look ─────────────────────────────────────────────────

/// The wallpaper on screen (awww, swww or hyprpaper), so the previews sit on the real thing.
fn wallpaper() -> Option<String> {
    let image = |p: &str| {
        let p = config::expand(p.trim());
        let lower = p.to_string_lossy().to_lowercase();
        ([".png", ".jpg", ".jpeg", ".webp"].iter().any(|e| lower.ends_with(e)) && p.is_file()).then(|| p.display().to_string())
    };
    for tool in ["awww", "swww"] {
        if let Ok(out) = std::process::Command::new(tool).arg("query").output() {
            let text = String::from_utf8_lossy(&out.stdout);
            if let Some(p) = text.lines().find_map(|l| l.split_once("image: ").and_then(|(_, p)| image(p))) {
                return Some(p);
            }
        }
    }
    let text = std::fs::read_to_string(dirs::config_dir()?.join("hypr/hyprpaper.conf")).ok()?;
    text.lines().filter(|l| !l.trim_start().starts_with('#')).flat_map(|l| l.split(['=', ','])).find_map(image)
}

fn preview_css(scope: &str, dark: bool, style: &str, opacity: f64, accent: &str) -> String {
    let fg = if dark { "rgba(255, 255, 255, 0.96)" } else { "#1d1d1f" };
    let dim = if dark { "rgba(235, 235, 245, 0.6)" } else { "rgba(60, 60, 67, 0.62)" };
    let accent = if theme::valid_color(accent) { accent } else if dark { "#ff9f0a" } else { "#f08a00" };
    // not .card: GTK themes style that class themselves
    let mut s = theme::card_css(&format!("{scope} .pv-card"), dark, style, opacity);
    s += &format!("{scope} .pv-card {{ color: {fg}; }} {scope} .pv-card .pv-dim {{ color: {dim}; }} {scope} .pv-card .pv-accent {{ color: {accent}; }} {scope} .pv-tick {{ background-color: {accent}; }}\n");
    if dark && style != "solid" && opacity < 0.75 {
        s += &format!("{scope} .pv-card label {{ text-shadow: 0 1px 2px rgba(0, 0, 0, 0.35); }}\n");
    }
    s
}

fn clock_card(big: bool) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    card.add_css_class("pv-card");
    card.add_css_class(if big { "pv-big" } else { "pv-small-card" });
    let now = chrono::Local::now();
    let date = gtk::Label::new(Some(&now.format("%A").to_string().to_uppercase()));
    date.add_css_class("pv-eyebrow");
    date.add_css_class("pv-accent");
    date.set_xalign(0.0);
    card.append(&date);
    let time = gtk::Label::new(Some(&now.format("%H:%M").to_string()));
    time.add_css_class(if big { "pv-time-big" } else { "pv-time" });
    time.set_xalign(0.0);
    card.append(&time);
    let sub = gtk::Label::new(Some(&now.format("%-d %B").to_string()));
    sub.add_css_class("pv-dim");
    sub.add_css_class("pv-small");
    sub.set_xalign(0.0);
    card.append(&sub);
    card
}

fn list_card() -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 6);
    card.add_css_class("pv-card");
    card.add_css_class("pv-big");
    let t = gtk::Label::new(Some("Reminders"));
    t.add_css_class("pv-title");
    t.add_css_class("pv-accent");
    t.set_xalign(0.0);
    card.append(&t);
    for (i, text) in ["Call the landlord", "Pick up the parcel", "Gym, push day"].iter().enumerate() {
        let r = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let tick = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        tick.add_css_class("pv-tickbox");
        if i == 1 {
            tick.add_css_class("pv-tick");
        }
        tick.set_valign(gtk::Align::Center);
        r.append(&tick);
        let l = gtk::Label::new(Some(text));
        l.add_css_class("pv-small");
        if i == 1 {
            l.add_css_class("pv-dim");
        }
        r.append(&l);
        card.append(&r);
    }
    card
}

fn wall(class: &str) -> gtk::Box {
    let w = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    w.add_css_class("wall");
    w.add_css_class(class);
    w
}

fn look_page(store: &Rc<Store>, cfg: &Config) -> gtk::Widget {
    let page = page_box();
    let display = gdk::Display::default().expect("no display");

    // previews of every preset, and one of the current look
    let fixed = gtk::CssProvider::new();
    let mut css = match wallpaper() {
        Some(p) => format!(".wall {{ background-image: url(\"file://{}\"); background-size: cover; background-position: center; }}\n", p.replace('"', "%22")),
        None => String::new(),
    };
    for (i, p) in PRESETS.iter().enumerate() {
        css += &preview_css(&format!(".pv-{i}"), p.appearance == "dark", p.style, p.opacity, "");
    }
    for (i, a) in ACCENTS.iter().enumerate().skip(1) {
        css += &format!(".swatch-{i} {{ background-color: {a}; }}\n");
    }
    fixed.load_from_string(&css);
    gtk::style_context_add_provider_for_display(&display, &fixed, gtk::STYLE_PROVIDER_PRIORITY_USER + 2);
    let live = gtk::CssProvider::new();
    gtk::style_context_add_provider_for_display(&display, &live, gtk::STYLE_PROVIDER_PRIORITY_USER + 2);

    // what's picked now
    #[derive(Clone)]
    struct Look {
        appearance: String,
        style: String,
        opacity: f64,
        accent: String,
    }
    let look = Rc::new(RefCell::new(Look {
        appearance: cfg.appearance.clone(),
        style: theme::style_class(&cfg.style).to_owned(),
        opacity: cfg.opacity,
        accent: cfg.accent.clone(),
    }));

    page.append(&heading("Preview"));
    let big = wall("pv-live");
    big.add_css_class("wall-big");
    big.append(&clock_card(true));
    big.append(&list_card());
    page.append(&big);
    page.append(&note("On your desktop Hyprland also blurs what's behind the cards, so they read a bit softer than here."));

    page.append(&heading("Themes"));
    let presets = gtk::FlowBox::new();
    presets.set_selection_mode(gtk::SelectionMode::None);
    presets.set_max_children_per_line(3);
    presets.set_min_children_per_line(3);
    presets.set_column_spacing(12);
    presets.set_row_spacing(12);
    presets.set_homogeneous(true);
    let mut preset_buttons = Vec::new();
    for (i, p) in PRESETS.iter().enumerate() {
        let b = gtk::Button::new();
        b.add_css_class("preset");
        b.add_css_class("flat");
        let v = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let w = wall(&format!("pv-{i}"));
        let c = clock_card(false);
        c.set_halign(gtk::Align::Center);
        c.set_hexpand(true);
        w.append(&c);
        v.append(&w);
        let l = gtk::Label::new(Some(p.name));
        l.add_css_class("preset-name");
        v.append(&l);
        b.set_child(Some(&v));
        presets.insert(&b, -1);
        preset_buttons.push(b);
    }
    page.append(&presets);

    page.append(&heading("Custom"));
    let g = group();
    let appearances = ["auto", "dark", "light"];
    let appearance = gtk::DropDown::from_strings(&["Follow the system", "Dark", "Light"]);
    appearance.set_selected(appearances.iter().position(|a| *a == cfg.appearance).unwrap_or(0) as u32);
    g.append(&row("Appearance", "Follow the system uses the GNOME color scheme", &appearance).0);
    let style = gtk::DropDown::from_strings(&["Glass", "Frosted", "Solid"]);
    style.set_selected(theme::STYLES.iter().position(|s| *s == theme::style_class(&cfg.style)).unwrap_or(0) as u32);
    g.append(&row("Style", "Glass has a light edge and a sheen, frosted is flat", &style).0);
    let opacity = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 0.05);
    opacity.set_value(cfg.opacity);
    opacity.set_width_request(240);
    opacity.set_draw_value(true);
    opacity.set_digits(2);
    opacity.add_mark(0.7, gtk::PositionType::Bottom, None);
    let (opacity_row, _) = row("Opacity", "Higher reads better on bright wallpapers", &opacity);
    g.append(&opacity_row);
    let swatches = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let mut swatch_buttons = Vec::new();
    for (i, a) in ACCENTS.iter().enumerate() {
        let b = gtk::Button::new();
        b.add_css_class("swatch");
        b.add_css_class(&format!("swatch-{i}"));
        b.set_tooltip_text(Some(if a.is_empty() { "Default" } else { a }));
        if a.is_empty() {
            b.set_label("A");
            b.add_css_class("swatch-auto");
        }
        swatches.append(&b);
        swatch_buttons.push(b);
    }
    g.append(&row("Accent", "Highlights and ticked reminders", &swatches).0);
    page.append(&g);

    // one place that shows the look and saves it
    let refresh = {
        let (look, live, presets, swatches, opacity_row) = (look.clone(), live.clone(), preset_buttons.clone(), swatch_buttons.clone(), opacity_row.clone());
        Rc::new(move || {
            let l = look.borrow();
            let dark = theme::is_dark(&l.appearance);
            live.load_from_string(&preview_css(".pv-live", dark, &l.style, l.opacity, &l.accent));
            for (b, p) in presets.iter().zip(PRESETS) {
                let appearance = if l.appearance == "auto" { if dark { "dark" } else { "light" } } else { l.appearance.as_str() };
                let on = p.appearance == appearance && p.style == l.style && (p.style == "solid" || (p.opacity - l.opacity).abs() < 0.01);
                if on { b.add_css_class("selected") } else { b.remove_css_class("selected") }
            }
            for (b, a) in swatches.iter().zip(ACCENTS) {
                if *a == l.accent { b.add_css_class("selected") } else { b.remove_css_class("selected") }
            }
            opacity_row.set_sensitive(l.style != "solid");
        })
    };
    refresh();
    let save = {
        let (store, look) = (store.clone(), look.clone());
        Rc::new(move || {
            let l = look.borrow().clone();
            store.set(None, "appearance", l.appearance);
            store.set(None, "style", l.style);
            store.set(None, "opacity", (l.opacity * 100.0).round() / 100.0);
            store.set(None, "accent", l.accent);
        })
    };
    // a preset sets three controls at once; save once, after the last
    let quiet = Rc::new(Cell::new(false));
    {
        let (look, refresh, save, quiet) = (look.clone(), refresh.clone(), save.clone(), quiet.clone());
        appearance.connect_selected_notify(move |d| {
            look.borrow_mut().appearance = appearances[d.selected() as usize].to_owned();
            refresh();
            if !quiet.get() { save() }
        });
    }
    {
        let (look, refresh, save, quiet) = (look.clone(), refresh.clone(), save.clone(), quiet.clone());
        style.connect_selected_notify(move |d| {
            look.borrow_mut().style = theme::STYLES[d.selected() as usize].to_owned();
            refresh();
            if !quiet.get() { save() }
        });
    }
    {
        let (look, refresh, quiet, store) = (look.clone(), refresh.clone(), quiet.clone(), store.clone());
        let save = save.clone();
        opacity.connect_value_changed(move |s| {
            look.borrow_mut().opacity = s.value();
            refresh();
            if !quiet.get() {
                let save = save.clone();
                // the widgets follow while dragging, a few times a second
                store.later("opacity", move |_| save());
            }
        });
    }
    for (b, a) in swatch_buttons.iter().zip(ACCENTS) {
        let (look, refresh, save) = (look.clone(), refresh.clone(), save.clone());
        b.connect_clicked(move |_| {
            look.borrow_mut().accent = (*a).to_owned();
            refresh();
            save();
        });
    }
    for (b, p) in preset_buttons.iter().zip(PRESETS) {
        let (appearance, style, opacity, quiet, save) = (appearance.clone(), style.clone(), opacity.clone(), quiet.clone(), save.clone());
        b.connect_clicked(move |_| {
            quiet.set(true);
            appearance.set_selected(appearances.iter().position(|a| *a == p.appearance).unwrap_or(0) as u32);
            style.set_selected(theme::STYLES.iter().position(|s| *s == p.style).unwrap_or(0) as u32);
            if p.style != "solid" {
                opacity.set_value(p.opacity);
            }
            quiet.set(false);
            save();
        });
    }
    page.upcast()
}

// ── Widgets ──────────────────────────────────────────────

const ABOUT: &[(&str, &str)] = &[
    ("clock", "Time and date"),
    ("weather", "Now, the next hours and the week, from Open-Meteo"),
    ("calendar", "A month with your events (Calendars page)"),
    ("agenda", "The same events as a list"),
    ("reminders", "Apple Reminders: tick off, add new ones (needs pyicloud)"),
    ("media", "What's playing, with controls"),
    ("pulse", "Health scores from a command (see the README)"),
];

fn widgets_page(store: &Rc<Store>, cfg: &Config) -> gtk::Widget {
    let page = page_box();
    page.append(&heading("On the desktop"));
    page.append(&note("Drag a card to move it, drag its corner or right-click it to change its size."));
    let g = group();
    let on = Rc::new(RefCell::new(cfg.widgets.clone()));
    for (name, _) in crate::widgets::REGISTRY {
        let about = ABOUT.iter().find(|(n, _)| n == name).map_or("", |(_, a)| a);
        let title = format!("{}{}", name[..1].to_uppercase(), &name[1..]);
        let (store, on2, name2) = (store.clone(), on.clone(), name.to_string());
        let s = switch(on.borrow().contains(&name.to_string()), move |active| {
            let mut list = on2.borrow_mut();
            list.retain(|n| *n != name2);
            if active {
                list.push(name2.clone());
            }
            store.set(None, "widgets", Array::from_iter(list.iter()));
        });
        g.append(&row(&title, about, &s).0);
    }
    page.append(&g);

    page.append(&heading("When"));
    let g = group();
    let store2 = store.clone();
    let show = dropdown(&["Only on empty workspaces", "Always"], usize::from(cfg.show == "always"), move |i| {
        store2.set(None, "show", if i == 1 { "always" } else { "empty" });
    });
    g.append(&row("Show the widgets", "", &show).0);
    page.append(&g);

    page.append(&heading("Clock"));
    let g = group();
    let s2 = store.clone();
    g.append(&row("Seconds", "", &switch(cfg.clock.seconds, move |v| s2.set(Some("clock"), "seconds", v))).0);
    let s2 = store.clone();
    g.append(&row("12-hour time", "", &switch(cfg.clock.twelve_hour, move |v| s2.set(Some("clock"), "twelve_hour", v))).0);
    page.append(&g);

    page.append(&heading("Weather"));
    let g = group();
    let place = gtk::Entry::new();
    place.set_text(&cfg.weather.location);
    place.set_width_chars(22);
    let s2 = store.clone();
    place.connect_changed(move |e| {
        let text = e.text().to_string();
        s2.later("location", move |s| s.set(Some("weather"), "location", text));
    });
    g.append(&row("Place", "Any town or city", &place).0);
    let s2 = store.clone();
    g.append(&row("Fahrenheit", "", &switch(cfg.weather.fahrenheit, move |v| s2.set(Some("weather"), "fahrenheit", v))).0);
    page.append(&g);

    page.append(&heading("Now playing"));
    let g = group();
    let s2 = store.clone();
    g.append(&row("Hide when nothing plays", "", &switch(cfg.media.hide_when_idle, move |v| s2.set(Some("media"), "hide_when_idle", v))).0);
    page.append(&g);
    page.upcast()
}

// ── Calendars ────────────────────────────────────────────

#[derive(Clone, Default)]
struct Source {
    name: String,
    url: String,
    color: String,
}

fn save_sources(store: &Store, list: &[Source]) {
    {
        let mut doc = store.doc.borrow_mut();
        let Some(cal) = doc.as_table_mut().entry("calendar").or_insert(toml_edit::table()).as_table_mut() else { return };
        let mut aot = ArrayOfTables::new();
        for s in list.iter().filter(|s| !s.url.trim().is_empty()) {
            let mut t = Table::new();
            t.insert("name", toml_edit::value(s.name.trim()));
            t.insert("url", toml_edit::value(s.url.trim()));
            if theme::valid_color(&s.color) {
                t.insert("color", toml_edit::value(&s.color));
            }
            aot.push(t);
        }
        if aot.is_empty() {
            cal.remove("sources");
        } else {
            cal.insert("sources", Item::ArrayOfTables(aot));
        }
    }
    store.save();
}

fn rgba_hex(c: &gdk::RGBA) -> String {
    let v = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", v(c.red()), v(c.green()), v(c.blue()))
}

fn calendars_page(store: &Rc<Store>, cfg: &Config) -> gtk::Widget {
    let page = page_box();
    page.append(&heading("Calendars"));
    page.append(&note(
        "Any iCal (.ics) link works. Google Calendar: Settings → your calendar → Integrate calendar → \
         Secret address in iCal format. iCloud: share the calendar as public. Outlook: Publish a calendar → ICS.",
    ));
    let list = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let sources: Rc<RefCell<Vec<Source>>> = Rc::new(RefCell::new(
        cfg.calendar.sources.iter().map(|s| Source { name: s.name.clone(), url: s.url.clone(), color: s.color.clone().unwrap_or_default() }).collect(),
    ));
    let rebuild: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::default();
    let draw = {
        let (list, sources, store, rebuild) = (list.clone(), sources.clone(), store.clone(), rebuild.clone());
        Rc::new(move || {
            while let Some(c) = list.first_child() {
                list.remove(&c);
            }
            let count = sources.borrow().len();
            if count == 0 {
                list.append(&note("No calendars yet."));
            }
            for i in 0..count {
                let s = sources.borrow()[i].clone();
                let r = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                r.add_css_class("group");
                r.add_css_class("cal-row");
                let color = gtk::ColorDialogButton::new(Some(gtk::ColorDialog::new()));
                if let Ok(c) = gdk::RGBA::parse(if s.color.is_empty() { "#0a84ff" } else { &s.color }) {
                    color.set_rgba(&c);
                }
                let name = gtk::Entry::new();
                name.set_placeholder_text(Some("Name"));
                name.set_text(&s.name);
                name.set_width_chars(12);
                let url = gtk::Entry::new();
                url.set_placeholder_text(Some("https://… .ics"));
                url.set_text(&s.url);
                url.set_hexpand(true);
                let remove = gtk::Button::from_icon_name("user-trash-symbolic");
                remove.add_css_class("flat");
                remove.set_tooltip_text(Some("Remove"));
                r.append(&color);
                r.append(&name);
                r.append(&url);
                r.append(&remove);
                list.append(&r);
                let edit = {
                    let (sources, store) = (sources.clone(), store.clone());
                    move |f: &dyn Fn(&mut Source)| {
                        if let Some(s) = sources.borrow_mut().get_mut(i) {
                            f(s);
                        }
                        let list = sources.borrow().clone();
                        store.later("calendars", move |st| save_sources(st, &list));
                    }
                };
                let e = edit.clone();
                color.connect_rgba_notify(move |b| {
                    let hex = rgba_hex(&b.rgba());
                    e(&|s| s.color = hex.clone());
                });
                let e = edit.clone();
                name.connect_changed(move |w| {
                    let t = w.text().to_string();
                    e(&|s| s.name = t.clone());
                });
                let e = edit;
                url.connect_changed(move |w| {
                    let t = w.text().to_string();
                    e(&|s| s.url = t.clone());
                });
                let (sources, store, rebuild) = (sources.clone(), store.clone(), rebuild.clone());
                remove.connect_clicked(move |_| {
                    sources.borrow_mut().remove(i);
                    save_sources(&store, &sources.borrow());
                    let f = rebuild.borrow().clone();
                    if let Some(f) = f {
                        glib::idle_add_local_once(move || f());
                    }
                });
            }
        })
    };
    *rebuild.borrow_mut() = Some(draw.clone());
    draw();
    page.append(&list);
    let add = gtk::Button::with_label("Add a calendar");
    add.set_halign(gtk::Align::Start);
    {
        let (sources, draw) = (sources.clone(), draw.clone());
        add.connect_clicked(move |_| {
            let n = sources.borrow().len();
            sources.borrow_mut().push(Source { name: if n == 0 { "Personal".into() } else { String::new() }, ..Default::default() });
            draw();
        });
    }
    page.append(&add);
    page.append(&note("Calendars are saved once a link is filled in. The link is a secret: anyone who has it can read the calendar."));
    page.upcast()
}

// ── Setup ────────────────────────────────────────────────

fn status_icon() -> gtk::Image {
    let i = gtk::Image::new();
    i.set_pixel_size(18);
    i
}

fn set_status(icon: &gtk::Image, ok: bool) {
    icon.set_icon_name(Some(if ok { "object-select-symbolic" } else { "dialog-warning-symbolic" }));
    icon.set_css_classes(&[if ok { "ok" } else { "warn" }]);
}

fn setup_page(store: &Rc<Store>, cfg: &Config) -> gtk::Widget {
    let _ = store;
    let page = page_box();
    page.append(&heading("Setup"));
    page.append(&note("Everything hyprwidgets needs, checked. The buttons are the same as running `hyprwidgets setup` in a terminal."));
    let hypr = setup::Hypr::detect();

    struct Line {
        icon: gtk::Image,
        sub: gtk::Label,
        button: gtk::Button,
    }
    let g = group();
    let line = |title: &str, button: &str| {
        let b = gtk::Button::with_label(button);
        let icon = status_icon();
        let (r, sub) = row(title, " ", &b);
        r.prepend(&icon);
        g.append(&r);
        Line { icon, sub, button: b }
    };
    let running = line("Running", "Start");
    let autostart = line("Starts with Hyprland", "Set up");
    let blur = line("Blur behind the cards", "Set up");
    let launcher = line("In your app launcher", "Add");
    let reminders = line("Apple Reminders", "Guide");
    page.append(&g);

    let lines = Rc::new((running, autostart, blur, launcher, reminders));
    let refresh = {
        let (lines, hypr) = (lines.clone(), hypr.as_ref().map(|h| (h.dir.clone(), h.lua)));
        let reminders_path = cfg.reminders.icloud.clone();
        Rc::new(move || {
            let (running, autostart, blur, launcher, reminders) = &*lines;
            let up = setup::running();
            set_status(&running.icon, up);
            running.sub.set_text(if up { "The widgets are running" } else { "Not running right now" });
            running.button.set_label(if up { "Restart" } else { "Start" });
            let h = setup::Hypr::detect();
            let (a, b) = h.as_ref().map_or((false, false), |h| (h.autostart(), h.blur()));
            set_status(&autostart.icon, a);
            set_status(&blur.icon, b);
            let file = h.as_ref().map(|h| h.own().display().to_string()).unwrap_or_default();
            autostart.sub.set_text(match (&hypr, a) {
                (None, _) => "No Hyprland config in ~/.config/hypr",
                (_, true) => "hyprwidgets starts when you log in",
                (_, false) => "Adds it to Hyprland's start, in a file of its own",
            });
            blur.sub.set_text(if b { "The frosted look is on" } else { "Without it the cards look flat on busy wallpapers" });
            autostart.button.set_visible(!a && hypr.is_some());
            blur.button.set_visible(!b && hypr.is_some());
            autostart.button.set_tooltip_text(Some(&file));
            let l = setup::launcher_installed();
            set_status(&launcher.icon, l);
            launcher.sub.set_text(if l { "Find it as Hyprwidgets and Hyprwidgets Settings" } else { "So you can start it and its settings from your launcher" });
            launcher.button.set_visible(!l);
            let p = config::expand(&reminders_path).is_file();
            set_status(&reminders.icon, p);
            reminders.sub.set_text(if p {
                "pyicloud is installed. Log in once with `icloud reminders lists` in a terminal"
            } else {
                "Needs pyicloud: uv tool install 'pyicloud[cli]'"
            });
        })
    };
    refresh();
    {
        let r = refresh.clone();
        lines.0.button.connect_clicked(move |_| {
            if setup::running() {
                let _ = crate::self_cmd().arg("restart").status();
            } else if let Err(e) = setup::start() {
                eprintln!("hyprwidgets: {e}");
            }
            let r = r.clone();
            glib::timeout_add_local_once(Duration::from_millis(1500), move || r());
        });
    }
    for b in [&lines.1.button, &lines.2.button] {
        let r = refresh.clone();
        b.connect_clicked(move |_| {
            if let Some(h) = setup::Hypr::detect() {
                if let Err(e) = h.apply() {
                    eprintln!("hyprwidgets: {e}");
                }
            }
            r();
        });
    }
    {
        let r = refresh.clone();
        lines.3.button.connect_clicked(move |_| {
            if let Err(e) = setup::install_launcher() {
                eprintln!("hyprwidgets: {e}");
            }
            r();
        });
    }
    lines.4.button.connect_clicked(|_| {
        gtk::UriLauncher::new("https://github.com/elyesghazel/hyprwidgets#5-apple-reminders").launch(None::<&gtk::Window>, gio::Cancellable::NONE, |_| {});
    });
    {
        // started or stopped from elsewhere
        let r = refresh.clone();
        glib::timeout_add_local(Duration::from_secs(3), move || {
            r();
            glib::ControlFlow::Continue
        });
    }

    page.append(&heading("More"));
    let g = group();
    let open = gtk::Button::with_label("Open");
    open.connect_clicked(|_| {
        let file = gio::File::for_path(config::path());
        gtk::FileLauncher::new(Some(&file)).launch(None::<&gtk::Window>, gio::Cancellable::NONE, |_| {});
    });
    g.append(&row("Config file", &config::path().display().to_string(), &open).0);
    let reset = gtk::Button::with_label("Reset");
    reset.add_css_class("destructive-action");
    reset.connect_clicked(|_| {
        let _ = std::fs::remove_file(crate::layout::path());
        let _ = crate::self_cmd().arg("restart").status();
    });
    g.append(&row("Card positions", "Put every card back in the corner it started in", &reset).0);
    let guide = gtk::Button::with_label("Open");
    guide.connect_clicked(|_| {
        gtk::UriLauncher::new(README).launch(None::<&gtk::Window>, gio::Cancellable::NONE, |_| {});
    });
    g.append(&row("Setup guide", "Calendars, Reminders, Pulse and styling in detail", &guide).0);
    page.append(&g);
    page.upcast()
}
