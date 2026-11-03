use chrono::Local;
use gtk4 as gtk;
use gtk4::prelude::*;

use super::{Ctx, Size, Widget, card, clear, label};
use crate::shared::Shared;
use crate::sources::reminders::{self, Cmd, Reminder, State};

pub struct Reminders {
    data: Shared<State>,
    cmd: async_channel::Sender<Cmd>,
}

pub fn new(ctx: &Ctx) -> Option<Box<dyn Widget>> {
    if !ctx.cfg.reminders.enabled {
        return None;
    }
    let data = Shared::new();
    let (tx, rx) = async_channel::unbounded();
    let cmd = reminders::watch(ctx.cfg.reminders.clone(), tx);
    data.feed(rx);
    Some(Box::new(Reminders { data, cmd }))
}

impl Widget for Reminders {
    fn title(&self) -> &'static str {
        "Reminders"
    }
    fn icon(&self) -> &'static str {
        "checkbox-checked-symbolic"
    }
    fn sizes(&self) -> &'static [Size] {
        &[Size::Large, Size::Medium, Size::Small]
    }
    fn build(&self, ctx: &Ctx, size: Size) -> gtk::Widget {
        build(ctx, &self.data, self.cmd.clone(), size)
    }
    fn summary(&self, _: &Ctx, l: &gtk::Label) {
        self.data.bind(l, |l, s| {
            l.set_text(&match (s.items.len(), &s.error) {
                (0, Some(_)) => "Can't load".into(),
                (0, None) => "All done".into(),
                (n, _) => format!("{n} open"),
            })
        });
    }
}

/// Open reminders with a tick circle each, and a field to add one.
/// Large: the list with tick circles and a field to add one. Medium: the first two with ticks.
/// Small: the count and the first titles.
fn build(hub: &Ctx, data: &Shared<State>, cmd: async_channel::Sender<Cmd>, size: Size) -> gtk::Widget {
    let c = card("reminders", size);

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let title = label("Reminders", "card-title orange");
    title.set_hexpand(true);
    let spinner = gtk::Spinner::new();
    let count = label("", "rem-count orange");
    header.append(&title);
    header.append(&spinner);
    header.append(&count);
    c.append(&header);

    let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
    list.add_css_class("rem-list");
    list.set_vexpand(true);
    c.append(&list);

    let entry = gtk::Entry::new();
    entry.set_placeholder_text(Some("New Reminder"));
    entry.add_css_class("rem-entry");
    entry.set_primary_icon_name(Some("list-add-symbolic"));
    {
        let cmd = cmd.clone();
        entry.connect_activate(move |e| {
            let text = e.text().trim().to_string();
            if !text.is_empty() {
                let _ = cmd.send_blocking(Cmd::Create(text));
                e.set_text("");
            }
        });
    }
    if size == Size::Large {
        c.append(&entry);
    }

    {
        let cmd = cmd.clone();
        super::on_click(&header, move || {
            let _ = cmd.send_blocking(Cmd::Refresh);
        });
    }

    let max = match size {
        Size::Large => hub.cfg.reminders.max_items.min(5),
        Size::Medium => 2,
        _ => 3,
    };
    let small = size == Size::Small;
    data.bind(&c, move |_, state| {
        if state.loading { spinner.start() } else { spinner.stop() }
        spinner.set_visible(state.loading);
        count.set_text(&state.items.len().to_string());
        clear(&list);
        if let Some(e) = &state.error {
            list.append(&label(e, "small red"));
        }
        if state.items.is_empty() && state.error.is_none() {
            let done = label(if state.loading { "Loading…" } else { "All done" }, "dim");
            done.set_xalign(0.5);
            done.set_vexpand(true);
            list.append(&done);
        }
        for r in state.items.iter().take(max) {
            if small {
                list.append(&label(&r.title, "rem-title small-row"));
            } else {
                list.append(&row(r, &cmd));
            }
        }
        if state.items.len() > max {
            list.append(&label(&format!("{} more", state.items.len() - max), "small dim more"));
        }
    });
    c.upcast()
}

fn row(r: &Reminder, cmd: &async_channel::Sender<Cmd>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("rem-row");

    let tick = gtk::Button::new();
    tick.add_css_class("rem-tick");
    tick.set_valign(gtk::Align::Start);
    tick.set_tooltip_text(Some("Mark as done"));
    let id = r.id.clone();
    let cmd = cmd.clone();
    let row_weak = row.downgrade();
    tick.connect_clicked(move |b| {
        b.add_css_class("done");
        b.set_sensitive(false);
        if let Some(row) = row_weak.upgrade() {
            row.add_css_class("leaving");
        }
        // let the fill animation play before the row goes
        let cmd = cmd.clone();
        let id = id.clone();
        gtk4::glib::timeout_add_local_once(std::time::Duration::from_millis(450), move || {
            let _ = cmd.send_blocking(Cmd::Complete(id));
        });
    });
    row.append(&tick);

    let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
    text.set_hexpand(true);
    text.append(&label(&r.title, "rem-title"));
    if let Some(due) = r.due {
        let now = Local::now();
        let days = (due.date_naive() - now.date_naive()).num_days();
        let when = match days {
            0 => format!("Today, {}", due.format("%H:%M")),
            1 => format!("Tomorrow, {}", due.format("%H:%M")),
            -1 => format!("Yesterday, {}", due.format("%H:%M")),
            2..=6 => due.format("%A, %H:%M").to_string(),
            _ => due.format("%-d %b %Y").to_string(),
        };
        let due_label = label(&when, "small dim");
        if due < now {
            due_label.add_css_class("red");
        }
        text.append(&due_label);
    }
    row.append(&text);
    row
}
