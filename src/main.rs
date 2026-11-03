//! hyprwidgets: interactive desktop widgets for Hyprland. One layer-shell surface per monitor
//! on the bottom layer, shown while that monitor's workspace has no windows.

mod config;
mod hypr;
mod shared;
mod sources;
mod widgets;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{gdk, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use config::Config;
use shared::Shared;
use widgets::{Hub, Size};

const CSS: &str = include_str!("style.css");

fn main() -> glib::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("init") => return init(),
        Some("--help" | "-h" | "help") => {
            println!(
                "hyprwidgets {}\n\nUsage:\n  hyprwidgets         run the widgets\n  hyprwidgets init    write a starter config to {}\n\nSetup guide: https://github.com/elyesghazel/hyprwidgets#setup",
                env!("CARGO_PKG_VERSION"),
                config::path().display()
            );
            return glib::ExitCode::SUCCESS;
        }
        Some("--version" | "-V") => {
            println!("hyprwidgets {}", env!("CARGO_PKG_VERSION"));
            return glib::ExitCode::SUCCESS;
        }
        _ => {}
    }

    let app = gtk::Application::builder()
        .application_id("ch.elyesghazel.hyprwidgets")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(activate);
    app.run_with_args::<&str>(&[])
}

fn init() -> glib::ExitCode {
    let path = config::path();
    if path.exists() {
        eprintln!("{} already exists", path.display());
        return glib::ExitCode::FAILURE;
    }
    let _ = std::fs::create_dir_all(config::dir());
    match std::fs::write(&path, config::DEFAULT) {
        Ok(()) => {
            println!("wrote {}", path.display());
            glib::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{}: {e}", path.display());
            glib::ExitCode::FAILURE
        }
    }
}

fn activate(app: &gtk::Application) {
    let cfg = config::load();
    load_css(&cfg);
    let hub = Rc::new(start_sources(cfg));

    // keep the app alive with no window open (all workspaces busy)
    std::mem::forget(app.hold());

    let windows: Rc<RefCell<HashMap<String, gtk::ApplicationWindow>>> = Rc::default();
    let state: Shared<hypr::Monitors> = Shared::new();
    let (tx, rx) = async_channel::unbounded();
    hypr::watch(tx);
    state.feed(rx);

    let rebuild = {
        let (app, hub, windows, state) = (app.clone(), hub.clone(), windows.clone(), state.clone());
        move || {
            let display = gdk::Display::default().expect("no display");
            let monitors = display.monitors();
            let mut old = std::mem::take(&mut *windows.borrow_mut());
            for i in 0..monitors.n_items() {
                let Some(mon) = monitors.item(i).and_downcast::<gdk::Monitor>() else { continue };
                let name = mon.connector().map(|c| c.to_string()).unwrap_or_default();
                if !hub.cfg.monitors.is_empty() && !hub.cfg.monitors.contains(&name) {
                    continue;
                }
                let win = old.remove(&name).unwrap_or_else(|| surface(&app, &hub, &mon));
                windows.borrow_mut().insert(name, win);
            }
            for (_, w) in old {
                w.destroy();
            }
            if let Some(m) = state.get() {
                apply(&hub.cfg, &windows.borrow(), &m);
            }
        }
    };
    rebuild();
    let display = gdk::Display::default().expect("no display");
    display.monitors().connect_items_changed(move |_, _, _, _| {
        // the connector name arrives a moment after the monitor itself
        let rebuild = rebuild.clone();
        glib::timeout_add_local_once(Duration::from_millis(300), rebuild);
    });

    {
        let (hub, windows) = (hub.clone(), windows.clone());
        let watcher = gtk::Label::new(None);
        state.bind(&watcher, move |_, m| apply(&hub.cfg, &windows.borrow(), m));
        // the binding lives as long as this widget does; keep it forever
        std::mem::forget(watcher);
    }
}

/// Show or hide every monitor's widgets for the current workspaces.
fn apply(cfg: &Config, windows: &HashMap<String, gtk::ApplicationWindow>, monitors: &hypr::Monitors) {
    for (name, win) in windows {
        let show = monitors.get(name).is_some_and(|(ws, count)| {
            (cfg.workspaces.is_empty() || cfg.workspaces.contains(ws)) && (cfg.show == "always" || *count == 0)
        });
        // Hyprland is not reachable (nested session, testing): just show them
        let show = show || monitors.is_empty();
        set_shown(win, show);
    }
}

/// Fade in and out with CSS; the surface is unmapped once hidden so it takes no input.
fn set_shown(win: &gtk::ApplicationWindow, show: bool) {
    let Some(root) = win.child() else { return };
    if show {
        if !win.is_visible() {
            win.set_visible(true);
            // nothing focused until clicked, or the reminder field glows on every show
            gtk::prelude::GtkWindowExt::set_focus(win, None::<&gtk::Widget>);
        }
        let root = root.clone();
        glib::idle_add_local_once(move || root.add_css_class("shown"));
    } else if root.has_css_class("shown") || win.is_visible() {
        root.remove_css_class("shown");
        let win = win.clone();
        glib::timeout_add_local_once(Duration::from_millis(220), move || {
            if !win.child().is_some_and(|r| r.has_css_class("shown")) {
                win.set_visible(false);
            }
        });
    }
}

fn surface(app: &gtk::Application, hub: &Hub, monitor: &gdk::Monitor) -> gtk::ApplicationWindow {
    let win = gtk::ApplicationWindow::new(app);
    win.add_css_class("hyprwidgets");
    win.add_css_class(THEME.with(|t| *t.borrow()));
    win.init_layer_shell();
    win.set_namespace(Some("hyprwidgets"));
    win.set_layer(Layer::Bottom);
    win.set_monitor(Some(monitor));
    // OnDemand: the "New Reminder" field can take the keyboard when clicked
    win.set_keyboard_mode(KeyboardMode::OnDemand);
    win.set_exclusive_zone(0);
    let cfg = &hub.cfg;
    let (top, bottom, left, right) = match cfg.position.as_str() {
        "top-right" => (true, false, false, true),
        "bottom-left" => (false, true, true, false),
        "bottom-right" => (false, true, false, true),
        "left" => (false, false, true, false),
        "right" => (false, false, false, true),
        "center" => (false, false, false, false),
        _ => (true, false, true, false),
    };
    for (edge, on) in [(Edge::Top, top), (Edge::Bottom, bottom), (Edge::Left, left), (Edge::Right, right)] {
        win.set_anchor(edge, on);
        win.set_margin(edge, cfg.margin);
    }

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("root");
    let column = gtk::Box::new(gtk::Orientation::Vertical, 16);
    column.add_css_class("column");
    root.append(&column);

    let mut pending_small: Option<gtk::Widget> = None;
    for name in &cfg.widgets {
        let Some(w) = widgets::build(name, hub) else {
            eprintln!("hyprwidgets: unknown widget \"{name}\"");
            continue;
        };
        if widgets::size(name) == Size::Small {
            match pending_small.take() {
                Some(first) => {
                    let pair = gtk::Box::new(gtk::Orientation::Horizontal, 16);
                    pair.append(&first);
                    pair.append(&w);
                    column.append(&pair);
                }
                None => pending_small = Some(w),
            }
        } else {
            if let Some(first) = pending_small.take() {
                column.append(&first);
            }
            column.append(&w);
        }
    }
    if let Some(first) = pending_small {
        column.append(&first);
    }

    win.set_child(Some(&root));
    win
}

fn start_sources(cfg: Config) -> Hub {
    let now = Shared::new();
    now.set(chrono::Local::now());
    {
        let now = now.clone();
        glib::timeout_add_local(Duration::from_millis(1000), move || {
            now.set(chrono::Local::now());
            glib::ControlFlow::Continue
        });
    }
    let uses = |w: &str| cfg.widgets.iter().any(|x| x == w);

    let weather = Shared::new();
    if uses("weather") {
        let (tx, rx) = async_channel::unbounded();
        sources::weather::watch(cfg.weather.clone(), tx);
        weather.feed(rx);
    }
    let agenda = Shared::new();
    if uses("calendar") {
        let (tx, rx) = async_channel::unbounded();
        sources::calendar::watch(cfg.calendar.clone(), tx);
        agenda.feed(rx);
    }
    let reminders = Shared::new();
    let reminders_cmd = (uses("reminders") && cfg.reminders.enabled).then(|| {
        let (tx, rx) = async_channel::unbounded();
        let cmd = sources::reminders::watch(cfg.reminders.clone(), tx);
        reminders.feed(rx);
        cmd
    });
    let media = Shared::new();
    if uses("media") {
        let (tx, rx) = async_channel::unbounded();
        sources::media::watch(tx);
        media.feed(rx);
    }
    Hub { cfg, now, weather, agenda, reminders, reminders_cmd, media }
}

fn load_css(cfg: &Config) {
    let dark = match cfg.appearance.as_str() {
        "light" => false,
        "dark" => true,
        _ => !std::process::Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "color-scheme"])
            .output()
            .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("light")),
    };
    let display = gdk::Display::default().expect("no display");
    let provider = gtk::CssProvider::new();
    provider.load_from_string(CSS);
    gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    // a user stylesheet goes on top
    let user = config::dir().join("style.css");
    if user.exists() {
        let p = gtk::CssProvider::new();
        p.load_from_path(&user);
        gtk::style_context_add_provider_for_display(&display, &p, gtk::STYLE_PROVIDER_PRIORITY_USER);
    }
    THEME.with(|t| *t.borrow_mut() = if dark { "dark" } else { "light" });
}

thread_local! {
    /// "dark" or "light", picked once at start.
    static THEME: RefCell<&'static str> = const { RefCell::new("dark") };
}
