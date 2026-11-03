//! `hyprwidgets settings`: the config in a window. Every change goes straight into config.toml
//! (comments kept) and the running widgets pick it up from there.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};
use toml_edit::{Array, ArrayOfTables, DocumentMut, Item, Table, Value};

use crate::config::{self, Config};
use crate::setup;
use crate::theme::{self, BUILTIN};

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

thread_local! {
    /// The palette the settings window is drawn in; follows the theme picked.
    static PALETTE: gtk::CssProvider = gtk::CssProvider::new();
}

/// Redraws the settings window in a config's colors.
fn repaint(cfg: &Config) {
    PALETTE.with(|p| p.load_from_string(&theme::define_colors(&theme::resolved(cfg), cfg.opacity)));
}

const PAGES: &[(&str, &str, &str)] = &[
    ("look", "Look", "\u{f53f}"),
    ("widgets", "Widgets", "\u{f009}"),
    ("calendars", "Calendars", "\u{f073}"),
    ("setup", "Setup", "\u{f013}"),
];

fn build(app: &gtk::Application, page: Option<&str>) {
    let display = gdk::Display::default().expect("no display");
    // above ~/.config/gtk-4.0/gtk.css, which some themes use to restyle everything
    PALETTE.with(|p| gtk::style_context_add_provider_for_display(&display, p, gtk::STYLE_PROVIDER_PRIORITY_USER + 2));
    let css = gtk::CssProvider::new();
    css.load_from_string(CSS);
    gtk::style_context_add_provider_for_display(&display, &css, gtk::STYLE_PROVIDER_PRIORITY_USER + 1);

    let store = Store::load();
    let cfg = config::load();
    repaint(&cfg);

    let win = gtk::ApplicationWindow::builder().application(app).title("hyprwidgets settings").default_width(920).default_height(660).build();
    win.add_css_class("hw-settings");

    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_hexpand(true);
    stack.set_vexpand(true);
    let built: [gtk::Widget; 4] = [look_page(&store, &cfg), widgets_page(&store, &cfg), calendars_page(&store, &cfg), setup_page(&store, &cfg)];
    for ((name, title, _), page) in PAGES.iter().zip(built) {
        let scroll = gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).child(&page).build();
        stack.add_titled(&scroll, Some(name), title);
    }
    let first = match page.filter(|p| PAGES.iter().any(|(n, _, _)| n == p)) {
        Some(p) => p,
        // first run, or not set up yet: open on Setup
        None if !config::path().exists() || !setup::running() => "setup",
        None => "look",
    };

    // the sidebar, drawn like zunder's: a marker, a glyph, the name
    let side = gtk::Box::new(gtk::Orientation::Vertical, 0);
    side.add_css_class("side");
    let brand = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    brand.add_css_class("brand");
    let caret = gtk::Label::new(Some("\u{276f}"));
    caret.add_css_class("caret");
    brand.append(&caret);
    let name = gtk::Label::new(Some("hyprwidgets"));
    name.add_css_class("brand-name");
    brand.append(&name);
    side.append(&brand);
    let list = gtk::ListBox::new();
    list.add_css_class("side-list");
    for (_, title, glyph) in PAGES {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        row.add_css_class("side-item");
        for (text, class) in [("\u{2502}", "marker"), (*glyph, "glyph"), (*title, "side-title")] {
            let l = gtk::Label::new(Some(text));
            l.add_css_class(class);
            row.append(&l);
        }
        list.append(&row);
    }
    let index = PAGES.iter().position(|(n, _, _)| *n == first).unwrap_or(0);
    list.select_row(list.row_at_index(index as i32).as_ref());
    stack.set_visible_child_name(first);
    {
        let stack = stack.clone();
        list.connect_row_selected(move |_, row| {
            if let Some(r) = row {
                stack.set_visible_child_name(PAGES[r.index() as usize].0);
            }
        });
    }
    list.set_vexpand(true);
    side.append(&list);
    let path = gtk::Label::new(Some(&format!("v{}  ·  ~/.config/hyprwidgets", env!("CARGO_PKG_VERSION"))));
    path.add_css_class("side-path");
    path.set_xalign(0.0);
    side.append(&path);

    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body.add_css_class("settings-body");
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

/// "Title ──────", like a zunder section.
fn heading(text: &str) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    b.add_css_class("page-head");
    let l = gtk::Label::new(Some(text));
    l.add_css_class("page-title");
    b.append(&l);
    let sep = gtk::Separator::new(gtk::Orientation::Horizontal);
    sep.set_hexpand(true);
    sep.set_valign(gtk::Align::Center);
    b.append(&sep);
    b
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
    r.add_css_class("setting");
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);
    let t = gtk::Label::new(Some(title));
    t.set_xalign(0.0);
    t.add_css_class("setting-title");
    text.append(&t);
    let s = gtk::Label::new(Some(subtitle));
    s.set_xalign(0.0);
    s.set_wrap(true);
    s.add_css_class("setting-help");
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

/// Colors for one theme tile, scoped to it.
fn tile_css(scope: &str, p: &theme::Palette, opacity: f64) -> String {
    let [bg, fg, _muted, faint, ghost, accent, green, ..] = p;
    format!(
        "{scope} .pv-card {{ background-color: alpha({bg}, {opacity:.2}); border-color: {ghost}; color: {fg}; }}\n\
         {scope} .pv-accent {{ color: {accent}; }}\n\
         {scope} .pv-dim {{ color: {faint}; }}\n\
         {scope} .pv-rule {{ background-color: {ghost}; }}\n\
         {scope} .pv-tick {{ background-color: {green}; border-color: {green}; }}\n\
         {scope} .pv-swatch-a {{ background-color: {accent}; }}\n\
         {scope} .pv-swatch-g {{ background-color: {green}; }}\n\
         {scope} .pv-swatch-f {{ background-color: {fg}; }}\n"
    )
}

fn pv_label(text: &str, classes: &[&str]) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    for c in classes {
        l.add_css_class(c);
    }
    l.set_xalign(0.0);
    l
}

fn pv_head(title: &str, right: &str) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    b.append(&pv_label(title, &["pv-accent", "pv-bold"]));
    let r = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    r.add_css_class("pv-rule");
    r.set_hexpand(true);
    r.set_valign(gtk::Align::Center);
    b.append(&r);
    if !right.is_empty() {
        b.append(&pv_label(right, &["pv-dim"]));
    }
    b
}

/// A clock card as on the desktop.
fn clock_card(big: bool) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 2);
    card.add_css_class("pv-card");
    if big {
        card.add_css_class("pv-big");
    }
    let now = chrono::Local::now();
    card.append(&pv_head(&now.format("%A").to_string(), &now.format("w%V").to_string()));
    card.append(&pv_label(&now.format("%H:%M").to_string(), &[if big { "pv-time-big" } else { "pv-time" }]));
    card.append(&pv_label(&now.format("%-d %B").to_string(), &["pv-dim"]));
    card
}

fn list_card() -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 6);
    card.add_css_class("pv-card");
    card.add_css_class("pv-big");
    card.append(&pv_head("Reminders", "3"));
    for (i, text) in ["Call the landlord", "Pick up the parcel", "Gym, push day"].iter().enumerate() {
        let r = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let tick = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        tick.add_css_class("pv-tickbox");
        if i == 1 {
            tick.add_css_class("pv-tick");
        }
        tick.set_valign(gtk::Align::Center);
        r.append(&tick);
        r.append(&pv_label(text, if i == 1 { &["pv-dim"] } else { &[] }));
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

/// Accents offered next to the theme's own.
const ACCENTS: &[&str] = &["", "#b4befe", "#cba6f7", "#89b4fa", "#94e2d5", "#a6e3a1", "#f9e2af", "#fab387", "#f38ba8"];

fn look_page(store: &Rc<Store>, cfg: &Config) -> gtk::Widget {
    let page = page_box();
    let display = gdk::Display::default().expect("no display");

    // every theme tile in its own colors
    let tiles_css = gtk::CssProvider::new();
    let mut css = match wallpaper() {
        Some(p) => format!(".wall {{ background-image: url(\"file://{}\"); background-size: cover; background-position: center; }}\n", p.replace('"', "%22")),
        None => String::new(),
    };
    let zunder = theme::zunder_theme_name();
    let mut names: Vec<(String, String)> = Vec::new();
    if let Some(z) = &zunder {
        names.push(("zunder".into(), format!("zunder · {z}")));
    }
    names.extend(BUILTIN.iter().map(|t| (t.name.to_owned(), t.name.to_owned())));
    for (i, (name, _)) in names.iter().enumerate() {
        css += &tile_css(&format!(".tile-{i}"), &theme::palette(name).0, 0.92);
    }
    for (i, a) in ACCENTS.iter().enumerate().skip(1) {
        css += &format!(".swatch-{i} {{ background-color: {a}; }}\n");
    }
    tiles_css.load_from_string(&css);
    gtk::style_context_add_provider_for_display(&display, &tiles_css, gtk::STYLE_PROVIDER_PRIORITY_USER + 3);

    page.append(&heading("Look"));
    page.append(&note("How it sits on your wallpaper. On the desktop Hyprland also blurs what's behind the cards."));
    let big = wall("pv-live");
    big.add_css_class("wall-big");
    big.append(&clock_card(true));
    big.append(&list_card());
    page.append(&big);

    page.append(&heading("Theme"));
    page.append(&note(if zunder.is_some() {
        "The same palettes as zunder. \"zunder\" follows whatever zunder is set to."
    } else {
        "The same palettes as zunder."
    }));
    let grid = gtk::FlowBox::new();
    grid.set_selection_mode(gtk::SelectionMode::None);
    grid.set_max_children_per_line(4);
    grid.set_min_children_per_line(3);
    grid.set_column_spacing(10);
    grid.set_row_spacing(10);
    grid.set_homogeneous(true);
    let current = Rc::new(RefCell::new(cfg.clone()));
    let mut tiles = Vec::new();
    for (i, (name, label)) in names.iter().enumerate() {
        let b = gtk::Button::new();
        b.add_css_class("tile");
        b.add_css_class(&format!("tile-{i}"));
        let v = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let card = gtk::Box::new(gtk::Orientation::Vertical, 4);
        card.add_css_class("pv-card");
        card.append(&pv_head("Clock", ""));
        card.append(&pv_label(&chrono::Local::now().format("%H:%M").to_string(), &["pv-time"]));
        let dots = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        for c in ["pv-swatch-a", "pv-swatch-g", "pv-swatch-f"] {
            let d = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            d.add_css_class("pv-swatch");
            d.add_css_class(c);
            dots.append(&d);
        }
        card.append(&dots);
        v.append(&card);
        let l = gtk::Label::new(Some(label));
        l.add_css_class("tile-name");
        l.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        v.append(&l);
        b.set_child(Some(&v));
        grid.insert(&b, -1);
        tiles.push((b, name.clone()));
    }
    page.append(&grid);

    page.append(&heading("Cards"));
    let opacity = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.3, 1.0, 0.05);
    opacity.set_value(cfg.opacity);
    opacity.set_width_request(240);
    opacity.set_draw_value(true);
    opacity.set_digits(2);
    page.append(&row("Opacity", "Higher reads better on bright wallpapers", &opacity).0);
    let swatches = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let mut swatch_buttons = Vec::new();
    for (i, a) in ACCENTS.iter().enumerate() {
        let b = gtk::Button::new();
        b.add_css_class("swatch");
        b.add_css_class(&format!("swatch-{i}"));
        b.set_tooltip_text(Some(if a.is_empty() { "The theme's own" } else { a }));
        if a.is_empty() {
            b.set_label("A");
            b.add_css_class("swatch-auto");
        }
        swatches.append(&b);
        swatch_buttons.push(b);
    }
    page.append(&row("Accent", "Headings, today, the progress bars", &swatches).0);

    let refresh = {
        let (current, tiles, swatches) = (current.clone(), tiles.clone(), swatch_buttons.clone());
        Rc::new(move || {
            let c = current.borrow();
            repaint(&c);
            for (b, name) in &tiles {
                if *name == c.theme { b.add_css_class("selected") } else { b.remove_css_class("selected") }
            }
            for (b, a) in swatches.iter().zip(ACCENTS) {
                if *a == c.accent { b.add_css_class("selected") } else { b.remove_css_class("selected") }
            }
        })
    };
    refresh();
    for (b, name) in &tiles {
        let (current, refresh, store, name) = (current.clone(), refresh.clone(), store.clone(), name.clone());
        b.connect_clicked(move |_| {
            current.borrow_mut().theme = name.clone();
            refresh();
            store.set(None, "theme", name.as_str());
        });
    }
    for (b, a) in swatch_buttons.iter().zip(ACCENTS) {
        let (current, refresh, store) = (current.clone(), refresh.clone(), store.clone());
        b.connect_clicked(move |_| {
            current.borrow_mut().accent = (*a).to_owned();
            refresh();
            store.set(None, "accent", *a);
        });
    }
    {
        let (current, refresh, store) = (current.clone(), refresh.clone(), store.clone());
        opacity.connect_value_changed(move |s| {
            let v = (s.value() * 100.0).round() / 100.0;
            current.borrow_mut().opacity = v;
            refresh();
            store.later("opacity", move |st| st.set(None, "opacity", v));
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

fn status_icon() -> gtk::Label {
    let l = gtk::Label::new(None);
    l.set_width_chars(2);
    l
}

fn set_status(icon: &gtk::Label, ok: bool) {
    icon.set_text(if ok { "\u{2713}" } else { "!" });
    icon.set_css_classes(&[if ok { "ok" } else { "warn" }]);
}

fn setup_page(store: &Rc<Store>, cfg: &Config) -> gtk::Widget {
    let _ = store;
    let page = page_box();
    page.append(&heading("Setup"));
    page.append(&note("Everything hyprwidgets needs, checked. The buttons are the same as running `hyprwidgets setup` in a terminal."));
    let hypr = setup::Hypr::detect();

    struct Line {
        icon: gtk::Label,
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
            blur.sub.set_text(if b { "The wallpaper behind the cards is blurred" } else { "Without it busy wallpapers show through the cards" });
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
