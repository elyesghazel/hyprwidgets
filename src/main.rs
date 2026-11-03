//! hyprwidgets: interactive desktop widgets for Hyprland. One layer-shell surface per monitor
//! on the bottom layer with the cards on a grid, shown while that monitor's workspace has no
//! windows.

mod board;
mod config;
mod hypr;
mod layout;
mod settings;
mod setup;
mod shared;
mod sources;
mod theme;
mod widgets;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};

use config::Config;
use shared::Shared;
use board::Board;
use layout::Layout;
use widgets::{Ctx, Widget};

const CSS: &str = include_str!("style.css");
/// Loaded under CSS, so our own rules still set label spacing.
const RESET: &str = include_str!("reset.css");

pub const APP_ID: &str = "ch.elyesghazel.hyprwidgets";

const HELP: &str = "Usage:
  hyprwidgets            run the widgets (already running: open the settings)
  hyprwidgets settings   look, widgets, calendars and setup in one window
                         (`settings setup` opens on that page)
  hyprwidgets setup      start with Hyprland, blur, launcher entry; then start
  hyprwidgets restart    restart the running widgets
  hyprwidgets quit       stop them
  hyprwidgets init       write a starter config";

fn main() -> glib::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("init") => return init(),
        Some("settings") => return settings::run(args.get(1).cloned()),
        Some("setup") => return setup::run(),
        Some(action @ ("restart" | "quit")) => return remote(action),
        Some("--help" | "-h" | "help") => {
            println!("hyprwidgets {}\n\n{HELP}\n\nConfig: {}", env!("CARGO_PKG_VERSION"), config::path().display());
            return glib::ExitCode::SUCCESS;
        }
        Some("--version" | "-V") => {
            println!("hyprwidgets {}", env!("CARGO_PKG_VERSION"));
            return glib::ExitCode::SUCCESS;
        }
        Some(other) => {
            eprintln!("hyprwidgets: unknown command \"{other}\"\n\n{HELP}");
            return glib::ExitCode::FAILURE;
        }
        None => {}
    }

    // after a restart, wait until the old process has let go of the name
    let restarted = std::env::var_os("HYPRWIDGETS_REEXEC").is_some();
    if restarted {
        // SAFETY: single-threaded at this point
        unsafe { std::env::remove_var("HYPRWIDGETS_REEXEC") };
        for _ in 0..40 {
            if !setup::running() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    // one instance: starting it again (from a launcher) opens the settings instead.
    // HYPRWIDGETS_NON_UNIQUE lets a test session run next to the real one.
    let flags = if std::env::var_os("HYPRWIDGETS_NON_UNIQUE").is_some() {
        gio::ApplicationFlags::NON_UNIQUE
    } else {
        gio::ApplicationFlags::empty()
    };
    let app = gtk::Application::builder().application_id(APP_ID).flags(flags).build();
    // someone else started the widgets meanwhile: leave it to them, without opening the settings
    if restarted && app.register(gio::Cancellable::NONE).is_ok() && app.is_remote() {
        return glib::ExitCode::SUCCESS;
    }
    app.connect_activate(activate);
    app.run_with_args::<&str>(&[])
}

/// Sends `action` to the running instance.
fn remote(action: &str) -> glib::ExitCode {
    let app = gio::Application::new(Some(APP_ID), gio::ApplicationFlags::empty());
    if app.register(gio::Cancellable::NONE).is_err() || !app.is_remote() {
        eprintln!("hyprwidgets isn't running");
        return glib::ExitCode::FAILURE;
    }
    app.activate_action(action, None);
    if let Some(c) = app.dbus_connection() {
        let _ = c.flush_sync(gio::Cancellable::NONE);
    }
    if action == "restart" {
        // back up before returning, so a `setup` right after doesn't start a second one
        drop(app);
        std::thread::sleep(Duration::from_millis(300));
        for _ in 0..30 {
            if setup::running() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    glib::ExitCode::SUCCESS
}

/// This program again, by the name it was started with so an updated binary is picked up.
pub fn self_cmd() -> std::process::Command {
    let arg0 = std::env::args().next().unwrap_or_else(|| "hyprwidgets".into());
    std::process::Command::new(arg0)
}

pub fn open_settings() {
    match self_cmd().arg("settings").spawn() {
        // reaped once closed, or it stays around as a zombie
        Ok(mut child) => drop(std::thread::spawn(move || child.wait())),
        Err(e) => eprintln!("hyprwidgets: can't open the settings: {e}"),
    }
}

/// Replaces this process with a fresh one: the config changed in a way that needs a rebuild.
fn reexec() {
    use std::os::unix::process::CommandExt;
    let err = self_cmd().env("HYPRWIDGETS_REEXEC", "1").exec();
    eprintln!("hyprwidgets: restart failed: {err}");
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
    /// The config as last loaded; only its look may differ from `ctx.cfg`.
    look: RefCell<Config>,
    theme: gtk::CssProvider,
    /// What `theme` holds.
    css: RefCell<String>,
}

thread_local! {
    static STARTED: Cell<bool> = const { Cell::new(false) };
}

fn activate(app: &gtk::Application) {
    // started again while running
    if STARTED.replace(true) {
        open_settings();
        return;
    }
    let cfg = config::load();
    let theme = load_css(&cfg);

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

    let me = Rc::new(App {
        app: app.clone(),
        look: RefCell::new(ctx.cfg.clone()),
        ctx: ctx.clone(),
        widgets: Rc::new(widgets),
        boards: RefCell::default(),
        monitors: Shared::new(),
        css: RefCell::new(theme::css(&ctx.cfg)),
        theme,
    });
    actions(app);
    watch_config(&me);
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
    let classes = theme::classes(&me.look.borrow());
    for i in 0..monitors.n_items() {
        let Some(mon) = monitors.item(i).and_downcast::<gdk::Monitor>() else { continue };
        let conn = mon.connector().map(|c| c.to_string()).unwrap_or_default();
        if !me.ctx.cfg.monitors.is_empty() && !me.ctx.cfg.monitors.contains(&conn) {
            continue;
        }
        let board = old.remove(&conn).unwrap_or_else(|| Board::new(&me.app, &me.ctx, &me.widgets, &mon, &classes));
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

fn actions(app: &gtk::Application) {
    let add = |name: &str, f: fn(&gtk::Application)| {
        let a = gio::SimpleAction::new(name, None);
        let app2 = app.clone();
        a.connect_activate(move |_, _| f(&app2));
        app.add_action(&a);
    };
    add("restart", |_| reexec());
    add("quit", |app| app.quit());
    add("settings", |_| open_settings());
}

/// Applies config.toml as soon as it's saved: the look in place, anything else by restarting.
/// zunder's config is watched too, for `theme = "zunder"`.
fn watch_config(me: &Rc<App>) {
    let _ = std::fs::create_dir_all(config::dir());
    let pending: Rc<Cell<Option<glib::SourceId>>> = Rc::default();
    let zunder = dirs::config_dir().unwrap_or_default().join("zunder/config.toml");
    for path in [config::path(), zunder] {
        let file = gio::File::for_path(path);
        let Ok(monitor) = file.monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE) else { continue };
        let (me, pending) = (me.clone(), pending.clone());
        monitor.connect_changed(move |_, _, _, _| {
            // editors write in several steps; act once it settles
            if let Some(id) = pending.take() {
                id.remove();
            }
            let (me, pending2) = (me.clone(), pending.clone());
            pending.set(Some(glib::timeout_add_local_once(Duration::from_millis(300), move || {
                pending2.set(None);
                reload(&me);
            })));
        });
        std::mem::forget(monitor);
    }
}

fn reload(me: &Rc<App>) {
    let new = match config::try_load() {
        Ok(c) => c,
        // half-written by hand: keep what's running
        Err(e) => return eprintln!("hyprwidgets: {e}"),
    };
    if !me.ctx.cfg.same_but_look(&new) {
        eprintln!("hyprwidgets: config changed, restarting");
        reexec();
        return;
    }
    let css = theme::css(&new);
    if *me.css.borrow() == css {
        return;
    }
    eprintln!("hyprwidgets: new look applied");
    me.theme.load_from_string(&css);
    *me.css.borrow_mut() = css;
    let classes = theme::classes(&new);
    for b in me.boards.borrow().values() {
        for c in ["dark", "light"] {
            b.win.remove_css_class(c);
        }
        for c in classes {
            b.win.add_css_class(c);
        }
    }
    *me.look.borrow_mut() = new;
}

/// Loads the stylesheets; returns the one generated from the config's look.
fn load_css(cfg: &Config) -> gtk::CssProvider {
    let display = gdk::Display::default().expect("no display");
    let reset = gtk::CssProvider::new();
    reset.load_from_string(RESET);
    gtk::style_context_add_provider_for_display(&display, &reset, gtk::STYLE_PROVIDER_PRIORITY_USER + 1);
    let provider = gtk::CssProvider::new();
    provider.load_from_string(CSS);
    // above ~/.config/gtk-4.0/gtk.css (USER priority), or a system theme like Catppuccin
    // paints the window background and restyles the buttons
    gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_USER + 2);
    let theme = gtk::CssProvider::new();
    theme.load_from_string(&theme::css(cfg));
    gtk::style_context_add_provider_for_display(&display, &theme, gtk::STYLE_PROVIDER_PRIORITY_USER + 3);
    // a user stylesheet goes on top
    let user = config::dir().join("style.css");
    if user.exists() {
        let p = gtk::CssProvider::new();
        p.load_from_path(&user);
        gtk::style_context_add_provider_for_display(&display, &p, gtk::STYLE_PROVIDER_PRIORITY_USER + 4);
    }
    theme
}
