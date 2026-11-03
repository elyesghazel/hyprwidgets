//! The cards. Every widget implements [`Widget`] and is listed once in [`REGISTRY`]; that is all it
//! takes to add one. A widget starts its own data source in `new` and builds one card per monitor
//! and size.

pub mod calendar;
pub mod clock;
pub mod media;
pub mod pulse;
pub mod reminders;
pub mod weather;

use std::rc::Rc;

use chrono::{DateTime, Local};
use gtk4 as gtk;
use gtk4::prelude::*;

use crate::config::Config;
use crate::layout::Layout;
use crate::shared::Shared;

/// What every widget gets: the config, a once-a-second clock tick, and the saved layout.
pub struct Ctx {
    pub cfg: Config,
    pub now: Shared<DateTime<Local>>,
    pub layout: Shared<Layout>,
}

impl Ctx {
    /// Change the layout, save it, and tell every card and the window manager.
    pub fn edit_layout(&self, f: impl FnOnce(&mut Layout)) {
        let mut l = self.layout.get().unwrap_or_default();
        f(&mut l);
        l.save();
        self.layout.set(l);
    }
}

pub use crate::layout::Size;

/// A desktop widget.
pub trait Widget {
    /// Shown on the collapsed card and in its menu.
    fn title(&self) -> &'static str;
    /// A symbolic icon name for the collapsed card.
    fn icon(&self) -> &'static str;
    /// The sizes it has a layout for; the first is where it starts.
    fn sizes(&self) -> &'static [Size];
    /// One card of `size` for one monitor. Built again when it's resized.
    fn build(&self, ctx: &Ctx, size: Size) -> gtk::Widget;
    /// Keeps the one-line text of the collapsed card up to date ("14:05", "4° Rain").
    fn summary(&self, _ctx: &Ctx, _label: &gtk::Label) {}
}

type Constructor = fn(&Ctx) -> Option<Box<dyn Widget>>;

/// Every widget there is, by the name used in `widgets = [...]`.
pub const REGISTRY: &[(&str, Constructor)] = &[
    ("clock", clock::new),
    ("weather", weather::new),
    ("calendar", calendar::new),
    ("agenda", calendar::new_agenda),
    ("reminders", reminders::new),
    ("media", media::new),
    ("pulse", pulse::new),
];

/// Starts the named widget (and its data source). None for unknown or disabled widgets.
pub fn start(name: &str, ctx: &Ctx) -> Option<Box<dyn Widget>> {
    let (_, new) = REGISTRY.iter().find(|(n, _)| *n == name)?;
    new(ctx)
}

/// A card in its frame: the card itself, the one-line pill it collapses to, and the controls
/// that show on hover.
pub struct Frame {
    pub root: gtk::Overlay,
    /// Bottom-right corner: drag to resize.
    pub handle: gtk::DrawingArea,
}

/// `collapse(name, on)` collapses or opens it.
pub fn frame(name: &str, w: &dyn Widget, ctx: &Rc<Ctx>, size: Size, collapse_fn: Rc<dyn Fn(&str, bool)>) -> Frame {
    let full = w.build(ctx, size);
    let (pw, _) = size.px();

    let pill = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    pill.add_css_class("card");
    pill.add_css_class("pill");
    pill.add_css_class(name);
    pill.set_size_request(pw, crate::layout::CELL);
    pill.set_valign(gtk::Align::Start);
    let icon = gtk::Image::from_icon_name(w.icon());
    icon.add_css_class("pill-icon");
    pill.append(&icon);
    if matches!(size, Size::Medium | Size::Large) {
        let t = label(w.title(), "pill-title");
        t.set_ellipsize(gtk4::pango::EllipsizeMode::None);
        t.set_max_width_chars(-1);
        pill.append(&t);
    }
    let summary = label("", "pill-summary dim");
    summary.set_hexpand(true);
    if matches!(size, Size::Medium | Size::Large) {
        summary.set_xalign(1.0);
    }
    w.summary(ctx, &summary);
    pill.append(&summary);
    {
        let (f, name) = (collapse_fn.clone(), name.to_string());
        on_click(&pill, move || f(&name, false));
    }

    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_transition_duration(160);
    stack.set_vhomogeneous(false);
    stack.set_interpolate_size(true);
    // the card's shadow reaches past it; clipped, it would show as a box around the card
    stack.set_overflow(gtk::Overflow::Visible);
    stack.add_named(&full, Some("full"));
    stack.add_named(&pill, Some("pill"));

    let collapse = gtk::Button::from_icon_name("pan-up-symbolic");
    collapse.add_css_class("chrome");
    collapse.set_tooltip_text(Some("Collapse"));
    collapse.set_halign(gtk::Align::End);
    collapse.set_valign(gtk::Align::Start);
    collapse.set_visible(size != Size::Mini);
    {
        let (f, name) = (collapse_fn.clone(), name.to_string());
        collapse.connect_clicked(move |_| f(&name, true));
    }

    // a rounded corner, like the one on iPad widgets in edit mode
    let handle = gtk::DrawingArea::new();
    handle.set_content_width(26);
    handle.set_content_height(26);
    handle.set_draw_func(|a, cr, w, h| {
        let (w, h) = (w as f64, h as f64);
        let c = a.color();
        cr.set_source_rgba(c.red() as f64, c.green() as f64, c.blue() as f64, 0.85);
        cr.set_line_width(3.5);
        cr.set_line_cap(gtk::cairo::LineCap::Round);
        let r = 11.0;
        cr.arc(w - 5.0 - r, h - 5.0 - r, r, 0.0, std::f64::consts::FRAC_PI_2);
        let _ = cr.stroke();
    });
    handle.add_css_class("resize-handle");
    handle.set_halign(gtk::Align::End);
    handle.set_valign(gtk::Align::End);
    handle.set_tooltip_text(Some("Drag to resize"));
    handle.set_cursor_from_name(Some("se-resize"));
    handle.set_visible(w.sizes().len() > 1);

    let root = gtk::Overlay::new();
    root.add_css_class("hw-frame");
    root.set_child(Some(&stack));
    root.add_overlay(&collapse);
    root.add_overlay(&handle);
    // a card that hides itself (media with nothing playing) takes its frame along
    full.bind_property("visible", &root, "visible").sync_create().build();

    // which page shows: also when the card shows itself again (a hidden page can't be the
    // visible one, so the stack falls back to the pill)
    let show: Rc<dyn Fn(bool)> = {
        let (stack, collapse, handle) = (stack.clone(), collapse.clone(), handle.clone());
        let resizable = w.sizes().len() > 1;
        Rc::new(move |collapsed: bool| {
            stack.set_visible_child_name(if collapsed { "pill" } else { "full" });
            collapse.set_visible(!collapsed && size != Size::Mini);
            handle.set_visible(!collapsed && resizable);
        })
    };
    {
        let (name, show) = (name.to_string(), show.clone());
        ctx.layout.bind(&stack, move |_, l| show(l.items.get(&name).is_some_and(|i| i.collapsed)));
    }
    {
        let (name, ctx) = (name.to_string(), ctx.clone());
        full.connect_visible_notify(move |_| {
            let collapsed = ctx.layout.get().unwrap_or_default().items.get(&name).is_some_and(|i| i.collapsed);
            show(collapsed);
        });
    }
    Frame { root, handle }
}

/// An empty card of the given size with its CSS class.
pub fn card(class: &str, size: Size) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 0);
    b.add_css_class("card");
    b.add_css_class(class);
    b.add_css_class(size.name());
    let (w, h) = size.px();
    b.set_size_request(w, h);
    b.set_overflow(gtk::Overflow::Hidden);
    b
}

pub fn label(text: &str, class: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    for c in class.split_whitespace() {
        l.add_css_class(c);
    }
    l.set_xalign(0.0);
    l.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    // ellipsized labels would otherwise ask for their whole text and widen the surface
    l.set_max_width_chars(1);
    l
}

pub fn open_url(url: &str) {
    let _ = std::process::Command::new("xdg-open")
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// Runs a shell command line without waiting for it.
pub fn run(cmd: &str) {
    let _ = std::process::Command::new("sh")
        .args(["-c", cmd])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// Run `f` on a plain click (not a drag), so cards stay clickable without being buttons.
pub fn on_click(w: &impl IsA<gtk::Widget>, f: impl Fn() + 'static) {
    let click = gtk::GestureClick::new();
    click.connect_released(move |g, _, _, _| {
        g.set_state(gtk::EventSequenceState::Claimed);
        f();
    });
    w.as_ref().add_controller(click);
    w.as_ref().set_cursor_from_name(Some("pointer"));
}

pub fn clear(b: &impl IsA<gtk::Widget>) {
    while let Some(c) = b.as_ref().first_child() {
        c.unparent();
    }
}

/// "14:05" or "2:05 PM".
pub fn time(t: &DateTime<Local>, twelve: bool) -> String {
    if twelve { t.format("%-I:%M %p").to_string() } else { t.format("%H:%M").to_string() }
}

/// Gives one widget its own background or text colour.
pub fn tint(w: &impl IsA<gtk::Widget>, css: &str) {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(&format!("* {{ {css} }}"));
    #[allow(deprecated)]
    w.as_ref().style_context().add_provider(&provider, gtk::STYLE_PROVIDER_PRIORITY_USER + 4);
}
