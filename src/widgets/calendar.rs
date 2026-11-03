//! The calendar in three sizes. Large: a month you can page through, days with events get dots,
//! clicking a day lists its events underneath, double-clicking it (or the month name) opens your
//! calendar. Medium: what's next, grouped by day. Small: today's date and the next event.

use std::cell::Cell;
use std::rc::Rc;

use chrono::{Datelike, Local, Months, NaiveDate};
use gtk4 as gtk;
use gtk4::prelude::*;

use super::{Ctx, Size, Widget, card, clear, label, on_click, open_url, run, time, tint};
use crate::sources::calendar::Event;
use crate::config;
use crate::shared::Shared;
use crate::sources::calendar::{self, Agenda};

pub struct Calendar {
    data: Shared<Agenda>,
    sizes: &'static [Size],
}

pub fn new(ctx: &Ctx) -> Option<Box<dyn Widget>> {
    Some(Box::new(Calendar { data: calendar::shared(&ctx.cfg.calendar), sizes: &[Size::Large, Size::Medium, Size::Small] }))
}

/// The same calendar, starting as the list of what's next.
pub fn new_agenda(ctx: &Ctx) -> Option<Box<dyn Widget>> {
    Some(Box::new(Calendar { data: calendar::shared(&ctx.cfg.calendar), sizes: &[Size::Medium, Size::Large, Size::Small] }))
}

impl Widget for Calendar {
    fn title(&self) -> &'static str {
        "Calendar"
    }
    fn icon(&self) -> &'static str {
        "x-office-calendar-symbolic"
    }
    fn sizes(&self) -> &'static [Size] {
        self.sizes
    }
    fn build(&self, ctx: &Ctx, size: Size) -> gtk::Widget {
        match size {
            Size::Large => month(ctx, &self.data),
            Size::Medium => agenda(ctx, &self.data),
            _ => today(ctx, &self.data),
        }
    }
    fn summary(&self, ctx: &Ctx, l: &gtk::Label) {
        next_up(&self.data, ctx.cfg.clock.twelve_hour, l);
    }
}

/// Opens the calendar app: `open_command` if set (e.g. the SUPER+K overlay), else the web address
/// for that day.
pub fn open(cfg: &config::Calendar, d: NaiveDate) {
    if !cfg.open_command.trim().is_empty() {
        run(&cfg.open_command);
    } else {
        open_url(&cfg.open.replace("{y}", &d.year().to_string()).replace("{m}", &d.month().to_string()).replace("{d}", &d.day().to_string()));
    }
}

struct View {
    /// First of the month on show.
    month: Cell<NaiveDate>,
    selected: Cell<NaiveDate>,
    today: Cell<NaiveDate>,
}

fn month(ctx: &Ctx, data: &Shared<Agenda>) -> gtk::Widget {
    let c = card("calendar", Size::Large);
    let today = Local::now().date_naive();
    let view = Rc::new(View { month: Cell::new(today.with_day(1).unwrap()), selected: Cell::new(today), today: Cell::new(today) });

    // header: "November 2026" on the left, ‹ • › on the right
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    header.add_css_class("cal-header");
    let title = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    title.set_hexpand(true);
    let month_name = label("", "card-title red");
    month_name.set_hexpand(false);
    month_name.set_max_width_chars(-1);
    month_name.set_ellipsize(gtk4::pango::EllipsizeMode::None);
    let year = label("", "card-title dim");
    year.set_max_width_chars(-1);
    year.set_ellipsize(gtk4::pango::EllipsizeMode::None);
    title.append(&month_name);
    title.append(&year);
    title.set_tooltip_text(Some("Open calendar"));
    header.append(&title);
    let nav = |icon: &str, tip: &str| {
        let b = gtk::Button::from_icon_name(icon);
        b.add_css_class("cal-nav");
        b.set_tooltip_text(Some(tip));
        header.append(&b);
        b
    };
    let prev = nav("go-previous-symbolic", "Previous month");
    let home = nav("media-record-symbolic", "Today");
    home.add_css_class("cal-home");
    let next = nav("go-next-symbolic", "Next month");
    c.append(&header);

    let grid = gtk::Grid::new();
    grid.add_css_class("cal-grid");
    grid.set_column_homogeneous(true);
    grid.set_row_spacing(2);
    c.append(&grid);

    let day_title = label("", "cal-heading dim");
    day_title.add_css_class("cal-day-title");
    c.append(&day_title);
    let list = gtk::Box::new(gtk::Orientation::Vertical, 4);
    list.add_css_class("cal-list");
    c.append(&list);

    let cfg = ctx.cfg.calendar.clone();
    let twelve = ctx.cfg.clock.twelve_hour;

    // the selected day's events
    let render_list: Rc<dyn Fn()> = {
        let (view, data, list, cfg, day_title) = (view.clone(), data.clone(), list.clone(), cfg.clone(), day_title.clone());
        Rc::new(move || {
            let msg_today = "No more events today";
            let agenda = data.get().unwrap_or_default();
            clear(&list);
            let sel = view.selected.get();
            let now = Local::now();
            let today = view.today.get();
            let mut events: Vec<_> = agenda.events.iter().filter(|e| e.on(sel)).collect();
            if sel == today {
                events.retain(|e| e.upcoming(&now));
            }
            // what's left of the card under five or six weeks
            let first = view.month.get();
            let lead = first.weekday().num_days_from_monday() as i64;
            let days = (first.checked_add_months(Months::new(1)).unwrap() - first).num_days();
            let room = if (lead + days + 6) / 7 >= 6 { 1 } else { 2 };
            day_title.set_text(&match (sel - today).num_days() {
                0 => "TODAY".into(),
                1 => "TOMORROW".into(),
                -1 => "YESTERDAY".into(),
                _ => sel.format("%A %-d %B").to_string().to_uppercase(),
            });
            if events.is_empty() {
                let msg = match (&agenda.error, sel == today) {
                    (Some(e), _) if agenda.events.is_empty() => e.clone(),
                    (_, true) => msg_today.into(),
                    _ => "No events".into(),
                };
                // today is done: show what comes next instead of an empty card
                match agenda.events.iter().find(|e| e.upcoming(&now) && e.start.date_naive() > today) {
                    Some(e) if sel == today => {
                        day_title.set_text(&format!("NOTHING ELSE TODAY · NEXT {}", e.start.format("%a %-d").to_string().to_uppercase()));
                    }
                    _ => list.append(&label(&msg, "small dim cal-empty")),
                }
                if sel == today
                    && let Some(e) = agenda.events.iter().find(|e| e.upcoming(&now) && e.start.date_naive() > today)
                {
                    let row = event_row(e, twelve, &now);
                    let (cfg, d) = (cfg.clone(), e.start.date_naive());
                    on_click(&row, move || open(&cfg, d));
                    list.append(&row);
                }
            }
            let shown = if events.len() > room { room - 1 } else { room };
            for e in events.iter().take(shown) {
                let row = event_row(e, twelve, &now);
                let (cfg, d) = (cfg.clone(), sel);
                on_click(&row, move || open(&cfg, d));
                list.append(&row);
            }
            if events.len() > shown {
                let more = label(&format!("{} more · double-click the day", events.len() - shown), "small dim more");
                list.append(&more);
            }
        })
    };

    // the month grid, then the list
    let render: Rc<dyn Fn()> = {
        let (view, data, grid, cfg, render_list) = (view.clone(), data.clone(), grid.clone(), cfg.clone(), render_list.clone());
        let (month_name, year) = (month_name.clone(), year.clone());
        let me: Rc<std::cell::RefCell<std::rc::Weak<dyn Fn()>>> = Rc::new(std::cell::RefCell::new(std::rc::Weak::<Box<dyn Fn()>>::new() as std::rc::Weak<dyn Fn()>));
        let f: Rc<dyn Fn()> = {
            let me = me.clone();
            Rc::new(move || {
                let agenda = data.get().unwrap_or_default();
                let first = view.month.get();
                month_name.set_text(&first.format("%B").to_string());
                year.set_text(&first.format("%Y").to_string());
                clear(&grid);
                for (i, d) in ["M", "T", "W", "T", "F", "S", "S"].iter().enumerate() {
                    let l = label(d, "cal-weekday dim");
                    l.set_xalign(0.5);
                    grid.attach(&l, i as i32, 0, 1, 1);
                }
                let lead = first.weekday().num_days_from_monday() as i64;
                let start = first - chrono::Duration::days(lead);
                let next_month = first.checked_add_months(Months::new(1)).unwrap();
                let weeks = (lead + (next_month - first).num_days() + 6) / 7;
                let pager = Pager { view: view.clone(), grid: grid.clone(), render: me.borrow().clone(), render_list: render_list.clone() };
                let pager = Rc::new(pager);
                for i in 0..weeks * 7 {
                    let d = start + chrono::Duration::days(i);
                    let cell = day_cell(d, first, &agenda, &cfg, &pager);
                    grid.attach(&cell, (i % 7) as i32, (i / 7 + 1) as i32, 1, 1);
                }
                render_list();
            })
        };
        *me.borrow_mut() = Rc::downgrade(&f);
        f
    };

    {
        let (view, render) = (view.clone(), render.clone());
        prev.connect_clicked(move |_| {
            view.month.set(view.month.get().checked_sub_months(Months::new(1)).unwrap());
            render();
        });
    }
    {
        let (view, render) = (view.clone(), render.clone());
        next.connect_clicked(move |_| {
            view.month.set(view.month.get().checked_add_months(Months::new(1)).unwrap());
            render();
        });
    }
    {
        let (view, render) = (view.clone(), render.clone());
        home.connect_clicked(move |_| {
            view.month.set(view.today.get().with_day(1).unwrap());
            view.selected.set(view.today.get());
            render();
        });
    }
    {
        let cfg = cfg.clone();
        on_click(&title, move || open(&cfg, Local::now().date_naive()));
    }

    {
        let render = render.clone();
        data.bind(&c, move |_, _| render());
    }
    // roll over at midnight, and keep "now" events current once a minute
    {
        let (view, render) = (view.clone(), render.clone());
        let last_min = Cell::new(u32::MAX);
        ctx.now.bind(&c, move |_, now| {
            let m = now.time().format("%H%M").to_string().parse().unwrap_or(0);
            if m == last_min.replace(m) {
                return;
            }
            let today = now.date_naive();
            if today != view.today.get() {
                if view.selected.get() == view.today.get() {
                    view.selected.set(today);
                }
                if view.month.get() == view.today.get().with_day(1).unwrap() {
                    view.month.set(today.with_day(1).unwrap());
                }
                view.today.set(today);
            }
            render();
        });
    }
    c.upcast()
}

/// What a day cell needs to change the selection or page to another month.
struct Pager {
    view: Rc<View>,
    grid: gtk::Grid,
    render: std::rc::Weak<dyn Fn()>,
    render_list: Rc<dyn Fn()>,
}

fn day_cell(d: NaiveDate, first: NaiveDate, agenda: &Agenda, cfg: &config::Calendar, pager: &Rc<Pager>) -> gtk::Widget {
    let view = &pager.view;
    let cell = gtk::Box::new(gtk::Orientation::Vertical, 1);
    cell.add_css_class("cal-cell");
    cell.set_halign(gtk::Align::Center);
    let num = gtk::Label::new(Some(&d.day().to_string()));
    num.add_css_class("cal-num");
    cell.append(&num);
    if d.month() != first.month() {
        cell.add_css_class("other");
    }
    if d.weekday().num_days_from_monday() >= 5 {
        cell.add_css_class("weekend");
    }
    if d == view.today.get() {
        cell.add_css_class("today");
    }
    if d == view.selected.get() {
        cell.add_css_class("selected");
    }

    // up to three dots in the calendars' colours
    let dots = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    dots.set_halign(gtk::Align::Center);
    dots.add_css_class("cal-dots");
    let mut colors: Vec<&str> = Vec::new();
    for e in agenda.events.iter().filter(|e| e.on(d)) {
        if !colors.contains(&e.color.as_str()) {
            colors.push(&e.color);
        }
    }
    for color in colors.iter().take(3) {
        let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        dot.add_css_class("cal-dot");
        tint(&dot, &format!("background-color: {color};"));
        dots.append(&dot);
    }
    cell.append(&dots);

    let click = gtk::GestureClick::new();
    let (pager, cfg) = (pager.clone(), cfg.clone());
    let me = cell.downgrade();
    click.connect_released(move |g, n, _, _| {
        g.set_state(gtk::EventSequenceState::Claimed);
        if n >= 2 {
            open(&cfg, d);
            return;
        }
        pager.view.selected.set(d);
        if d.month() != pager.view.month.get().month() {
            // a day of the next or previous month: page there (not from inside this cell's handler)
            pager.view.month.set(d.with_day(1).unwrap());
            if let Some(r) = pager.render.upgrade() {
                gtk4::glib::idle_add_local_once(move || r());
            }
            return;
        }
        // same month: move the highlight, keep the cells so a double click still lands
        let mut child = pager.grid.first_child();
        while let Some(c) = child {
            c.remove_css_class("selected");
            child = c.next_sibling();
        }
        if let Some(me) = me.upgrade() {
            me.add_css_class("selected");
        }
        (pager.render_list)();
    });
    cell.add_controller(click);
    cell.set_cursor_from_name(Some("pointer"));
    cell.upcast()
}

/// The collapsed line: the next event and when.
pub fn next_up(data: &Shared<Agenda>, twelve: bool, l: &gtk::Label) {
    data.bind(l, move |l, a| {
        let now = Local::now();
        l.set_text(&match a.events.iter().find(|e| e.upcoming(&now)) {
            None => "Nothing coming up".into(),
            Some(e) => {
                let when = if e.all_day {
                    String::new()
                } else if e.start <= now {
                    " · now".into()
                } else if e.start.date_naive() == now.date_naive() {
                    format!(" · {}", time(&e.start, twelve))
                } else {
                    format!(" · {}", e.start.format("%a"))
                };
                format!("{}{when}", e.title)
            }
        });
    });
}

/// Medium. Left: today's weekday and date. Right: the next events, grouped by day.
fn agenda(hub: &Ctx, data: &Shared<Agenda>) -> gtk::Widget {
    let c = card("agenda", Size::Medium);
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    row.set_vexpand(true);
    c.append(&row);

    let today = gtk::Box::new(gtk::Orientation::Vertical, 0);
    today.add_css_class("cal-today");
    let weekday = label("", "eyebrow red");
    let day = label("", "cal-day");
    day.set_ellipsize(gtk4::pango::EllipsizeMode::None);
    day.set_max_width_chars(-1);
    let summary = label("", "small dim");
    summary.set_wrap(true);
    summary.set_ellipsize(gtk4::pango::EllipsizeMode::None);
    summary.set_max_width_chars(10);
    let spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    spacer.set_vexpand(true);
    today.append(&weekday);
    today.append(&day);
    today.append(&spacer);
    today.append(&summary);
    row.append(&today);

    let list = gtk::Box::new(gtk::Orientation::Vertical, 6);
    list.set_hexpand(true);
    list.add_css_class("cal-list");
    row.append(&list);

    let cal = hub.cfg.calendar.clone();
    let open_day = move |d: NaiveDate| open(&cal, d);
    {
        let open_day = open_day.clone();
        super::on_click(&today, move || open_day(Local::now().date_naive()));
    }

    hub.now.bind(&c, move |_, now| {
        weekday.set_text(&now.format("%A").to_string().to_uppercase());
        day.set_text(&now.day().to_string());
    });

    let twelve = hub.cfg.clock.twelve_hour;
    // two days' headings and two events fill a medium card
    let max = hub.cfg.calendar.max_events.min(2);
    data.bind(&c, move |_, agenda| {
        clear(&list);
        let now = Local::now();
        let upcoming: Vec<&Event> = agenda.events.iter().filter(|e| e.upcoming(&now)).collect();
        let today_left = upcoming.iter().filter(|e| e.start.date_naive() <= now.date_naive()).count();
        summary.set_text(&match (today_left, &agenda.error) {
            (_, Some(e)) if upcoming.is_empty() => e.clone(),
            (0, _) => "No more events today".into(),
            (1, _) => "1 event today".into(),
            (n, _) => format!("{n} events today"),
        });
        if upcoming.is_empty() {
            let empty = label("Nothing coming up", "dim");
            empty.set_vexpand(true);
            empty.set_xalign(0.5);
            list.append(&empty);
            return;
        }
        let mut last_day = None;
        for e in upcoming.iter().take(max) {
            let d = e.start.date_naive().max(now.date_naive());
            if last_day != Some(d) {
                let heading = match (d - now.date_naive()).num_days() {
                    0 => "TODAY".to_string(),
                    1 => "TOMORROW".to_string(),
                    _ => e.start.format("%A %-d").to_string().to_uppercase(),
                };
                let h = label(&heading, "cal-heading dim");
                if last_day.is_some() {
                    h.add_css_class("gap");
                }
                list.append(&h);
                last_day = Some(d);
            }
            let item = event_row(e, twelve, &now);
            let open_day = open_day.clone();
            super::on_click(&item, move || open_day(d));
            list.append(&item);
        }
    });
    c.upcast()
}

pub fn event_row(e: &Event, twelve: bool, now: &chrono::DateTime<Local>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.add_css_class("cal-event");
    row.set_tooltip_text(Some(&e.calendar));
    let bar = gtk::Box::new(gtk::Orientation::Vertical, 0);
    bar.add_css_class("cal-bar");
    bar.set_size_request(3, -1);
    let provider = gtk::CssProvider::new();
    provider.load_from_string(&format!("box {{ background: {}; }}", e.color));
    #[allow(deprecated)]
    bar.style_context().add_provider(&provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    row.append(&bar);

    let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
    text.set_hexpand(true);
    text.append(&label(&e.title, "cal-title"));
    let when = if e.all_day {
        "All day".to_string()
    } else if e.start <= *now {
        format!("Now · until {}", time(&e.end, twelve))
    } else {
        format!("{} – {}", time(&e.start, twelve), time(&e.end, twelve))
    };
    let sub = if e.location.is_empty() { when } else { format!("{when} · {}", e.location) };
    text.append(&label(&sub, "small dim"));
    row.append(&text);
    row
}

/// Small: the date, big, and what's next today.
fn today(ctx: &Ctx, data: &Shared<Agenda>) -> gtk::Widget {
    let c = card("calendar-today", Size::Small);
    let weekday = label("", "eyebrow red");
    let day = label("", "cal-day");
    day.set_ellipsize(gtk4::pango::EllipsizeMode::None);
    day.set_max_width_chars(-1);
    let spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    spacer.set_vexpand(true);
    let next = gtk::Box::new(gtk::Orientation::Vertical, 0);
    c.append(&weekday);
    c.append(&day);
    c.append(&spacer);
    c.append(&next);
    let cal = ctx.cfg.calendar.clone();
    on_click(&c, move || open(&cal, Local::now().date_naive()));
    ctx.now.bind(&c, move |_, now| {
        weekday.set_text(&now.format("%A").to_string().to_uppercase());
        day.set_text(&now.day().to_string());
    });
    let twelve = ctx.cfg.clock.twelve_hour;
    data.bind(&c, move |_, a| {
        clear(&next);
        let now = Local::now();
        match a.events.iter().find(|e| e.upcoming(&now)) {
            None => next.append(&label("Nothing coming up", "small dim")),
            Some(e) => {
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                let bar = gtk::Box::new(gtk::Orientation::Vertical, 0);
                bar.add_css_class("cal-bar");
                bar.set_size_request(3, -1);
                tint(&bar, &format!("background-color: {};", e.color));
                let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
                text.set_hexpand(true);
                text.append(&label(&e.title, "cal-title"));
                let when = if e.all_day {
                    "All day".to_string()
                } else if e.start <= now {
                    "Now".to_string()
                } else if e.start.date_naive() == now.date_naive() {
                    time(&e.start, twelve)
                } else {
                    format!("{} {}", e.start.format("%a"), time(&e.start, twelve))
                };
                text.append(&label(&when, "small dim"));
                row.append(&bar);
                row.append(&text);
                next.append(&row);
            }
        }
    });
    c.upcast()
}
