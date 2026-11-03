//! hyprwidgets: interactive desktop widgets for Hyprland. One layer-shell surface per monitor
//! on the bottom layer with the cards on a grid, shown while that monitor's workspace has no
//! windows.

mod board;
mod config;
mod hypr;
mod layout;
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

use config::Config;
use shared::Shared;
use board::Board;
use layout::Layout;
use widgets::{Ctx, Widget};

const CSS: &str = include_str!("style.css");
/// Loaded under CSS, so our own rules still set label spacing.
const RESET: &str = include_str!("reset.css");

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

/// Everything that lives as long as the app.
struct App {
    app: gtk::Application,
    ctx: Rc<Ctx>,
    widgets: board::Widgets,
    /// One board per monitor connector.
    boards: RefCell<HashMap<String, Rc<Board>>>,
    monitors: Shared<hypr::Monitors>,
}

fn activate(app: &gtk::Application) {
    let cfg = config::load();
    load_css(&cfg);

    let now = Shared::new();
    now.set(chrono::Local::now());
    {
        let now = now.clone();
        glib::timeout_add_local(Duration::from_millis(1000), move || {
            now.set(chrono::Local::now());
            glib::ControlFlow::Continue
        });
    }
    let layout = Shared::new();
    layout.set(Layout::load());
    let ctx = Rc::new(Ctx { cfg, now, layout });

    let mut widgets: Vec<(String, Box<dyn Widget>)> = Vec::new();
    for name in &ctx.cfg.widgets {
        if widgets.iter().any(|(n, _)| n == name) {
            continue;
        }
        match widgets::start(name, &ctx) {
            Some(w) => widgets.push((name.clone(), w)),
            None if widgets::REGISTRY.iter().any(|(n, _)| n == name) => {}
            None => eprintln!("hyprwidgets: unknown widget \"{name}\""),
        }
    }

    // keep the app alive with no window open (all workspaces busy)
    std::mem::forget(app.hold());

    let me = Rc::new(App { app: app.clone(), ctx: ctx.clone(), widgets: Rc::new(widgets), boards: RefCell::default(), monitors: Shared::new() });
    let (tx, rx) = async_channel::unbounded();
    hypr::watch(tx);
    me.monitors.feed(rx);

    rebuild(&me);
    let display = gdk::Display::default().expect("no display");
    {
        let me = me.clone();
        display.monitors().connect_items_changed(move |_, _, _, _| {
            // the connector name arrives a moment after the monitor itself
            let me = me.clone();
            glib::timeout_add_local_once(Duration::from_millis(300), move || rebuild(&me));
        });
    }

    // bindings live as long as the widget they're bound to; this one is kept forever
    let keeper = gtk::Label::new(None);
    {
        let me = me.clone();
        me.clone().monitors.bind(&keeper, move |_, m| apply(&me, m));
    }
    {
        // moved, resized or collapsed somewhere: every monitor follows
        let me = me.clone();
        ctx.layout.bind(&keeper, move |_, _| {
            for b in me.boards.borrow().values() {
                b.sync();
            }
        });
    }
    std::mem::forget(keeper);
}

/// Makes sure every monitor has a board, and only those.
fn rebuild(me: &Rc<App>) {
    let display = gdk::Display::default().expect("no display");
    let monitors = display.monitors();
    let mut old = std::mem::take(&mut *me.boards.borrow_mut());
    let style = match me.ctx.cfg.style.as_str() {
        "frosted" => "frosted",
        "solid" => "solid",
        _ => "glass",
    };
    let theme = THEME.with(|t| *t.borrow());
    for i in 0..monitors.n_items() {
        let Some(mon) = monitors.item(i).and_downcast::<gdk::Monitor>() else { continue };
        let conn = mon.connector().map(|c| c.to_string()).unwrap_or_default();
        if !me.ctx.cfg.monitors.is_empty() && !me.ctx.cfg.monitors.contains(&conn) {
            continue;
        }
        let board = old.remove(&conn).unwrap_or_else(|| Board::new(&me.app, &me.ctx, &me.widgets, &mon, &[theme, style]));
        me.boards.borrow_mut().insert(conn, board);
    }
    for (_, b) in old {
        b.win.destroy();
    }
    if let Some(m) = me.monitors.get() {
        apply(me, &m);
    }
}

/// Show or hide every monitor's widgets for the current workspaces.
fn apply(me: &App, monitors: &hypr::Monitors) {
    let cfg = &me.ctx.cfg;
    for (name, board) in me.boards.borrow().iter() {
        let show = monitors.get(name).is_some_and(|(ws, count)| {
            (cfg.workspaces.is_empty() || cfg.workspaces.contains(ws)) && (cfg.show == "always" || *count == 0)
        });
        // Hyprland is not reachable (nested session, testing): just show them
        let show = show || monitors.is_empty();
        set_shown(&board.win, show);
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
    let reset = gtk::CssProvider::new();
    reset.load_from_string(RESET);
    gtk::style_context_add_provider_for_display(&display, &reset, gtk::STYLE_PROVIDER_PRIORITY_USER + 1);
    let provider = gtk::CssProvider::new();
    provider.load_from_string(CSS);
    // above ~/.config/gtk-4.0/gtk.css (USER priority), or a system theme like Catppuccin
    // paints the window background and restyles the buttons
    gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_USER + 2);
    // a user stylesheet goes on top
    let user = config::dir().join("style.css");
    if user.exists() {
        let p = gtk::CssProvider::new();
        p.load_from_path(&user);
        gtk::style_context_add_provider_for_display(&display, &p, gtk::STYLE_PROVIDER_PRIORITY_USER + 3);
    }
    THEME.with(|t| *t.borrow_mut() = if dark { "dark" } else { "light" });
}

thread_local! {
    /// "dark" or "light", picked once at start.
    static THEME: RefCell<&'static str> = const { RefCell::new("dark") };
}
