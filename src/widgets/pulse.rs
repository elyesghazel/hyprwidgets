//! Recovery, sleep and strain as three rings, the way Pulse shows them, plus the numbers behind.

use std::cell::Cell;
use std::f64::consts::PI;
use std::rc::Rc;

use gtk4 as gtk;
use gtk4::prelude::*;

use super::{Ctx, Size, Widget, card, label, on_click, open_url};
use crate::shared::Shared;
use crate::sources::pulse::{self, Scores};

pub struct Pulse {
    data: Shared<Scores>,
}

pub fn new(ctx: &Ctx) -> Option<Box<dyn Widget>> {
    let data = Shared::new();
    let (tx, rx) = async_channel::unbounded();
    pulse::watch(ctx.cfg.pulse.clone(), tx);
    data.feed(rx);
    Some(Box::new(Pulse { data }))
}

impl Widget for Pulse {
    fn title(&self) -> &'static str {
        "Pulse"
    }
    fn icon(&self) -> &'static str {
        "emblem-favorite-symbolic"
    }
    fn sizes(&self) -> &'static [Size] {
        &[Size::Medium, Size::Mini, Size::Small, Size::Large]
    }
    fn build(&self, ctx: &Ctx, size: Size) -> gtk::Widget {
        build(ctx, &self.data, size)
    }
    fn summary(&self, _: &Ctx, l: &gtk::Label) {
        self.data.bind(l, |l, s| {
            l.set_text(&match (s.recovery, s.sleep_min) {
                (Some(r), Some(m)) => format!("Recovery {r:.0}% · {}", hm(m)),
                (Some(r), None) => format!("Recovery {r:.0}%"),
                _ => s.error.clone().unwrap_or_else(|| "No data yet".into()),
            })
        });
    }
}

const GREEN: (f64, f64, f64) = (0.65, 0.89, 0.63);
const YELLOW: (f64, f64, f64) = (0.98, 0.89, 0.69);
const RED: (f64, f64, f64) = (0.95, 0.55, 0.66);
const SLEEP: (f64, f64, f64) = (0.71, 0.75, 1.0);
const STRAIN: (f64, f64, f64) = (0.98, 0.70, 0.53);

/// "7h34" from minutes.
fn hm(min: f64) -> String {
    let m = min.round() as i64;
    format!("{}h{:02}", m / 60, m % 60)
}

/// A ring with its value in the middle and a caption below.
struct Ring {
    root: gtk::Box,
    area: gtk::DrawingArea,
    value: gtk::Label,
    caption: gtk::Label,
    fraction: Rc<Cell<f64>>,
    color: Rc<Cell<(f64, f64, f64)>>,
}

fn ring(name: &str, color: (f64, f64, f64), px: i32) -> Ring {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
    root.set_hexpand(true);
    let fraction = Rc::new(Cell::new(0.0_f64));
    let color = Rc::new(Cell::new(color));
    let area = gtk::DrawingArea::new();
    area.set_content_width(px);
    area.set_content_height(px);
    area.set_halign(gtk::Align::Center);
    {
        let (fraction, color) = (fraction.clone(), color.clone());
        area.set_draw_func(move |a, cr, w, h| {
            let (w, h) = (w as f64, h as f64);
            let lw = (w.min(h) / 11.0).clamp(5.0, 10.0);
            let r = w.min(h) / 2.0 - lw / 2.0 - 1.0;
            let (cx, cy) = (w / 2.0, h / 2.0);
            let (red, green, blue) = color.get();
            let fg = a.color();
            cr.set_line_width(lw);
            cr.set_line_cap(gtk::cairo::LineCap::Round);
            cr.set_source_rgba(fg.red() as f64, fg.green() as f64, fg.blue() as f64, 0.12);
            cr.arc(cx, cy, r, 0.0, 2.0 * PI);
            let _ = cr.stroke();
            let f = fraction.get().clamp(0.0, 1.0);
            if f > 0.0 {
                cr.set_source_rgb(red, green, blue);
                cr.arc(cx, cy, r, -PI / 2.0, -PI / 2.0 + f.max(0.01) * 2.0 * PI);
                let _ = cr.stroke();
            }
        });
    }
    let value = label("–", "pulse-value");
    value.set_xalign(0.5);
    value.set_max_width_chars(-1);
    value.set_ellipsize(gtk4::pango::EllipsizeMode::None);
    value.set_halign(gtk::Align::Center);
    value.set_valign(gtk::Align::Center);
    let over = gtk::Overlay::new();
    over.set_child(Some(&area));
    over.add_overlay(&value);
    over.set_halign(gtk::Align::Center);
    root.append(&over);
    let title = label(name, "eyebrow dim");
    title.set_xalign(0.5);
    title.set_visible(!name.is_empty());
    root.append(&title);
    let caption = label("", "tiny dim");
    caption.set_xalign(0.5);
    caption.set_visible(!name.is_empty());
    root.append(&caption);
    Ring { root, area, value, caption, fraction, color }
}

impl Ring {
    fn set(&self, fraction: f64, value: &str, caption: &str) {
        self.fraction.set(fraction);
        self.value.set_text(value);
        self.caption.set_text(caption);
        self.area.queue_draw();
    }
}

/// Pulse's driver names, as people say them.
fn driver(label: &str) -> &str {
    match label {
        "HEART_RATE_VARIABILITY" => "HRV",
        "RESTING_HEART_RATE" => "Resting HR",
        "SLEEP_QUALITY" => "Sleep",
        "RESPIRATORY_RATE" => "Breathing",
        "SKIN_TEMPERATURE" => "Skin temp",
        other => other,
    }
}

/// Mini: the recovery ring. Small: recovery big, sleep and strain under it. Medium: the three
/// rings. Large: the rings, the numbers behind them and what moved recovery.
fn build(ctx: &Ctx, data: &Shared<Scores>, size: Size) -> gtk::Widget {
    let c = card("pulse", size);
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    header.add_css_class("head");
    let title = label("Pulse", "head-title");
    title.set_max_width_chars(-1);
    let when = label("", "small dim");
    when.set_xalign(1.0);
    when.set_ellipsize(gtk4::pango::EllipsizeMode::None);
    when.set_max_width_chars(-1);
    header.append(&title);
    header.append(&super::rule());
    header.append(&when);

    let px = match size {
        Size::Mini => 54,
        Size::Small => 88,
        Size::Medium => 68,
        Size::Large => 84,
    };
    let named = matches!(size, Size::Medium | Size::Large);
    let recovery = ring(if named { "recovery" } else { "" }, GREEN, px);
    let sleep = ring(if named { "sleep" } else { "" }, SLEEP, px);
    let strain = ring(if named { "strain" } else { "" }, STRAIN, px);
    let rings = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    rings.add_css_class("pulse-rings");
    rings.set_homogeneous(true);
    rings.set_vexpand(true);
    rings.set_valign(gtk::Align::Center);

    let stats = label("", "small dim pulse-stats");
    stats.set_xalign(0.5);
    let why = label("", "small pulse-why");
    why.set_xalign(0.5);
    let side = label("", "small pulse-side");
    side.set_xalign(0.5);
    // Large: the numbers behind the rings, two by two
    let facts = gtk::Grid::new();
    facts.add_css_class("pulse-facts");
    facts.set_column_homogeneous(true);
    facts.set_row_spacing(10);
    facts.set_column_spacing(12);
    facts.set_vexpand(true);
    facts.set_valign(gtk::Align::Center);
    let fact = |i: i32, name: &str| {
        let b = gtk::Box::new(gtk::Orientation::Vertical, 0);
        b.add_css_class("pulse-fact");
        b.append(&label(name, "eyebrow dim"));
        let v = label("–", "pulse-fact-value");
        b.append(&v);
        facts.attach(&b, i % 2, i / 2, 1, 1);
        v
    };
    let f_rhr = fact(0, "RESTING HR");
    let f_hrv = fact(1, "HRV");
    let f_energy = fact(2, "ENERGY");
    let f_debt = fact(3, "SLEEP DEBT");

    match size {
        Size::Mini => {
            recovery.value.add_css_class("tiny-value");
            c.append(&recovery.root);
            recovery.root.set_vexpand(true);
            recovery.root.set_valign(gtk::Align::Center);
        }
        Size::Small => {
            recovery.value.add_css_class("big-value");
            c.append(&recovery.root);
            recovery.root.set_vexpand(true);
            c.append(&side);
        }
        Size::Medium => {
            for r in [&recovery, &sleep, &strain] {
                rings.append(&r.root);
            }
            c.append(&rings);
            c.append(&stats);
        }
        Size::Large => {
            c.append(&header);
            for r in [&recovery, &sleep, &strain] {
                rings.append(&r.root);
            }
            rings.set_vexpand(false);
            c.append(&rings);
            c.append(&facts);
            c.append(&why);
        }
    }

    let open = ctx.cfg.pulse.open.clone();
    if !open.is_empty() {
        on_click(&c, move || open_url(&open));
    }

    data.bind(&c, move |_, s| {
        if let Some(e) = &s.error
            && s.recovery.is_none()
        {
            when.set_text("");
            stats.set_text(e);
            side.set_text(e);
            why.set_text("");
            for r in [&recovery, &sleep, &strain] {
                r.set(0.0, "–", "");
            }
            return;
        }
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        when.set_text(if s.day == today || s.day.is_empty() { "Today" } else { &s.day });

        let color = match s.band.as_deref() {
            Some("red") => RED,
            Some("yellow") => YELLOW,
            _ => GREEN,
        };
        recovery.color.set(color);
        match s.recovery {
            Some(r) => recovery.set(r / 100.0, &format!("{r:.0}%"), &format!("HRV {}", s.hrv.map_or("–".into(), |v| format!("{v:.0}")))),
            None => recovery.set(0.0, "–", "Not scored yet"),
        }
        match (s.sleep_min, s.sleep_perf) {
            (Some(m), perf) => sleep.set(
                perf.unwrap_or(0.0) / 100.0,
                &hm(m),
                &perf.map_or(String::new(), |p| format!("{p:.0}% of need")),
            ),
            _ => sleep.set(0.0, "–", ""),
        }
        match s.strain {
            Some(v) => strain.set(
                v / 21.0,
                &format!("{v:.1}"),
                &match (s.strain_low, s.strain_high) {
                    (Some(l), Some(h)) => format!("Target {l:.0}–{h:.0}"),
                    _ => String::new(),
                },
            ),
            None => strain.set(0.0, "–", ""),
        }

        let mut parts = Vec::new();
        if let Some(r) = s.rhr {
            parts.push(format!("RHR {r:.0}"));
        }
        if let Some(e) = s.energy {
            parts.push(format!("Energy {e:.0}%"));
        }
        if let Some(d) = s.debt_min.filter(|d| *d >= 15.0) {
            parts.push(format!("Sleep debt {}", hm(d)));
        }
        stats.set_text(&parts.join("  ·  "));
        let or_dash = |v: Option<String>| v.unwrap_or_else(|| "–".into());
        f_rhr.set_text(&or_dash(s.rhr.map(|v| format!("{v:.0} bpm"))));
        f_hrv.set_text(&or_dash(s.hrv.map(|v| format!("{v:.0} ms"))));
        f_energy.set_text(&or_dash(s.energy.map(|v| format!("{v:.0}%"))));
        f_debt.set_text(&or_dash(s.debt_min.map(hm)));
        let mut small = Vec::new();
        if let Some(m) = s.sleep_min {
            small.push(format!("Sleep {}", hm(m)));
        }
        if let Some(v) = s.strain {
            small.push(format!("Strain {v:.1}"));
        }
        side.set_text(&small.join(" · "));

        let mut reasons = Vec::new();
        if let Some(h) = &s.help {
            reasons.push(format!("▲ {} +{:.0}", driver(&h.label), h.points));
        }
        if let Some(d) = &s.drag {
            reasons.push(format!("▼ {} {:.0}", driver(&d.label), d.points));
        }
        why.set_text(&reasons.join("   "));
        why.set_visible(!reasons.is_empty());
    });
    c.upcast()
}
