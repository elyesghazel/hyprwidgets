//! One transparent surface per monitor, covering the desktop, with the cards on a grid. Drag a
//! card to move it (it follows the pointer and snaps to the nearest free cells), drag its corner
//! to switch to another size, or right-click it for the size menu. Only the cards take input; the
//! rest of the surface lets clicks through.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::time::Duration;

use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{gdk, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::layout::{GAP, Grid, Item, PITCH, Size};
use crate::widgets::{self, Ctx, Frame, Widget};

pub type Widgets = Rc<Vec<(String, Box<dyn Widget>)>>;

pub struct Board {
    pub win: gtk::ApplicationWindow,
    fixed: gtk::Fixed,
    /// Shows where a dragged card will land.
    ghost: gtk::Box,
    ctx: Rc<Ctx>,
    widgets: Widgets,
    monitor: gdk::Monitor,
    cards: RefCell<HashMap<String, (Frame, Size)>>,
    gesture: RefCell<Option<Gesture>>,
}

#[derive(Clone)]
struct Gesture {
    name: String,
    resize: bool,
    item: Item,
    /// The card's top-left when the drag started.
    origin: (f64, f64),
    active: bool,
    target: Option<Item>,
}

impl Board {
    pub fn new(app: &gtk::Application, ctx: &Rc<Ctx>, widgets: &Widgets, monitor: &gdk::Monitor, classes: &[&str]) -> Rc<Board> {
        let win = gtk::ApplicationWindow::new(app);
        win.add_css_class("hyprwidgets");
        for c in classes {
            win.add_css_class(c);
        }
        win.init_layer_shell();
        win.set_namespace(Some("hyprwidgets"));
        win.set_layer(Layer::Bottom);
        win.set_monitor(Some(monitor));
        // OnDemand: the "New Reminder" field can take the keyboard when clicked
        win.set_keyboard_mode(KeyboardMode::OnDemand);
        win.set_exclusive_zone(0);
        for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            win.set_anchor(edge, true);
        }

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("root");
        let fixed = gtk::Fixed::new();
        fixed.add_css_class("board");
        fixed.set_hexpand(true);
        fixed.set_vexpand(true);
        let m = ctx.cfg.margin;
        fixed.set_margin_top(m);
        fixed.set_margin_bottom(m);
        fixed.set_margin_start(m);
        fixed.set_margin_end(m);
        root.append(&fixed);
        win.set_child(Some(&root));

        let ghost = gtk::Box::new(gtk::Orientation::Vertical, 0);
        ghost.add_css_class("ghost");
        ghost.set_visible(false);
        ghost.set_can_target(false);
        fixed.put(&ghost, 0.0, 0.0);

        let board = Rc::new(Board {
            win,
            fixed,
            ghost,
            ctx: ctx.clone(),
            widgets: widgets.clone(),
            monitor: monitor.clone(),
            cards: RefCell::default(),
            gesture: RefCell::default(),
        });
        board.gestures();
        board.sync();
        {
            let b = Rc::downgrade(&board);
            board.win.connect_map(move |_| {
                if let Some(b) = b.upgrade() {
                    b.update_input_region();
                }
            });
        }
        board
    }

    /// The grid this monitor fits.
    fn grid(&self) -> Grid {
        let (w, h) = (self.fixed.width(), self.fixed.height());
        if w > 0 && h > 0 {
            return Grid::fitting(w, h);
        }
        // not laid out yet: the monitor minus the margins and roughly a bar
        let g = self.monitor.geometry();
        let m = self.ctx.cfg.margin;
        Grid::fitting(g.width() - 2 * m, g.height() - 2 * m - 50)
    }

    /// Where every card sits on this monitor: the saved spot, or the next free one.
    fn items(&self) -> BTreeMap<String, Item> {
        let saved = self.ctx.layout.get().unwrap_or_default().items;
        let grid = self.grid();
        let mut out: BTreeMap<String, Item> = BTreeMap::new();
        let mut pending = Vec::new();
        for (name, w) in self.widgets.iter() {
            match saved.get(name) {
                Some(it) if w.sizes().contains(&it.size) => {
                    let others: Vec<Item> = out.values().copied().collect();
                    // a smaller monitor than the one it was placed on, or a clash
                    let it = if grid.free(it, &others) { Some(*it) } else { grid.nearest_free(*it, &others) };
                    match it {
                        Some(it) => {
                            out.insert(name.clone(), it);
                        }
                        None => pending.push(name),
                    }
                }
                _ => pending.push(name),
            }
        }
        for name in pending {
            let w = self.widget(name);
            let others: Vec<Item> = out.values().copied().collect();
            out.insert(name.clone(), grid.auto_place(w.sizes()[0], &self.ctx.cfg.position, &others));
        }
        out
    }

    fn widget(&self, name: &str) -> &dyn Widget {
        self.widgets.iter().find(|(n, _)| n == name).map(|(_, w)| w.as_ref()).expect("known widget")
    }

    /// Puts every card where the layout says, rebuilding the ones whose size changed.
    pub fn sync(self: &Rc<Self>) {
        let items = self.items();
        let mut cards = self.cards.borrow_mut();
        for (name, it) in &items {
            let stale = cards.get(name).is_some_and(|(_, s)| *s != it.size);
            if stale && let Some((f, _)) = cards.remove(name) {
                self.fixed.remove(&f.root);
            }
            let (x, y) = ((it.col * PITCH) as f64, (it.row * PITCH) as f64);
            match cards.get(name) {
                Some((f, _)) => self.fixed.move_(&f.root, x, y),
                None => {
                    let me = Rc::downgrade(self);
                    let collapse: Rc<dyn Fn(&str, bool)> = Rc::new(move |name: &str, on: bool| {
                        let Some(me) = me.upgrade() else { return };
                        let name = name.to_string();
                        glib::idle_add_local_once(move || {
                            if let Some(it) = me.items().get(&name).copied() {
                                me.commit(&name, Item { collapsed: on, ..it });
                            }
                        });
                    });
                    let f = widgets::frame(name, self.widget(name), &self.ctx, it.size, collapse);
                    self.fixed.put(&f.root, x, y);
                    cards.insert(name.clone(), (f, it.size));
                }
            }
        }
        drop(cards);
        let b = Rc::downgrade(self);
        // after the cards have their new sizes (collapsing animates for 160 ms)
        glib::timeout_add_local_once(Duration::from_millis(250), move || {
            if let Some(b) = b.upgrade() {
                b.update_input_region();
            }
        });
    }

    /// Clicks outside the cards go to whatever is below.
    fn update_input_region(&self) {
        let Some(surface) = self.win.surface() else { return };
        let region = gtk::cairo::Region::create();
        for (f, _) in self.cards.borrow().values() {
            if !f.root.is_visible() {
                continue;
            }
            if let Some(b) = f.root.compute_bounds(&self.win) {
                let _ = region.union_rectangle(&gtk::cairo::RectangleInt::new(
                    b.x() as i32 - 4,
                    b.y() as i32 - 4,
                    b.width() as i32 + 8,
                    b.height() as i32 + 8,
                ));
            }
        }
        surface.set_input_region(Some(&region));
    }

    /// Which card (by name) a point on the board belongs to, and whether it's on its resize
    /// corner. None for text fields and buttons, which keep their own clicks and drags.
    fn hit(&self, x: f64, y: f64) -> Option<(String, bool)> {
        let mut w = self.fixed.pick(x, y, gtk::PickFlags::DEFAULT)?;
        let mut on_handle = false;
        loop {
            if w.is::<gtk::Entry>() || w.is::<gtk::Text>() || w.is::<gtk::Button>() {
                return None;
            }
            let parent = w.parent()?;
            if parent == *self.fixed.upcast_ref::<gtk::Widget>() {
                break;
            }
            for (f, _) in self.cards.borrow().values() {
                if w == *f.handle.upcast_ref::<gtk::Widget>() {
                    on_handle = true;
                }
            }
            w = parent;
        }
        let cards = self.cards.borrow();
        let (name, _) = cards.iter().find(|(_, (f, _))| w == *f.root.upcast_ref::<gtk::Widget>())?;
        Some((name.clone(), on_handle))
    }

    fn gestures(self: &Rc<Self>) {
        // the gestures sit on the board, not the card: the card moves under the pointer, so its
        // own coordinates would shift mid-drag
        let drag = gtk::GestureDrag::new();
        {
            let b = Rc::downgrade(self);
            drag.connect_drag_begin(move |g, x, y| {
                let Some(b) = b.upgrade() else { return };
                let hit = b.hit(x, y);
                let items = b.items();
                let Some(((name, resize), item)) = hit.and_then(|h| items.get(&h.0).copied().map(|i| (h, i))) else {
                    g.set_state(gtk::EventSequenceState::Denied);
                    return;
                };
                let origin = ((item.col * PITCH) as f64, (item.row * PITCH) as f64);
                *b.gesture.borrow_mut() = Some(Gesture { name, resize, item, origin, active: false, target: None });
            });
        }
        {
            let b = Rc::downgrade(self);
            drag.connect_drag_update(move |g, dx, dy| {
                let Some(b) = b.upgrade() else { return };
                let Some(mut gs) = b.gesture.borrow().clone() else { return };
                if !gs.active {
                    if dx.hypot(dy) < 8.0 {
                        return;
                    }
                    gs.active = true;
                    g.set_state(gtk::EventSequenceState::Claimed);
                    if let Some((f, _)) = b.cards.borrow().get(&gs.name) {
                        f.root.add_css_class(if gs.resize { "resizing" } else { "lifted" });
                        // on top of the others while it moves
                        f.root.insert_before(&b.fixed, None::<&gtk::Widget>);
                    }
                    b.ghost.insert_after(&b.fixed, None::<&gtk::Widget>);
                    b.ghost.set_visible(true);
                }
                let grid = b.grid();
                let others: Vec<Item> = b.items().into_iter().filter(|(n, _)| *n != gs.name).map(|(_, i)| i).collect();
                let want = if gs.resize {
                    let (w0, h0) = gs.item.size.px();
                    let (cw, ch) = (((w0 as f64 + dx + GAP as f64) / PITCH as f64), ((h0 as f64 + dy + GAP as f64) / PITCH as f64));
                    // the supported size nearest to what the pointer outlines
                    let size = *b
                        .widget(&gs.name)
                        .sizes()
                        .iter()
                        .min_by(|a, z| {
                            let d = |s: &Size| {
                                let (w, h) = s.cells();
                                (w as f64 - cw).powi(2) + (h as f64 - ch).powi(2)
                            };
                            d(a).total_cmp(&d(z))
                        })
                        .unwrap();
                    Item { size, collapsed: false, ..gs.item }
                } else {
                    let (x, y) = (gs.origin.0 + dx, gs.origin.1 + dy);
                    if let Some((f, _)) = b.cards.borrow().get(&gs.name) {
                        b.fixed.move_(&f.root, x.max(0.0), y.max(0.0));
                    }
                    Item { col: (x / PITCH as f64).round() as i32, row: (y / PITCH as f64).round() as i32, ..gs.item }
                };
                gs.target = if grid.free(&want, &others) { Some(want) } else { grid.nearest_free(want, &others) };
                if let Some(t) = gs.target {
                    let (w, h) = t.span();
                    b.ghost.set_size_request(w * PITCH - GAP, h * PITCH - GAP);
                    b.fixed.move_(&b.ghost, (t.col * PITCH) as f64, (t.row * PITCH) as f64);
                }
                *b.gesture.borrow_mut() = Some(gs);
            });
        }
        {
            let b = Rc::downgrade(self);
            drag.connect_drag_end(move |_, _, _| {
                let Some(b) = b.upgrade() else { return };
                let Some(gs) = b.gesture.borrow_mut().take() else { return };
                if !gs.active {
                    return;
                }
                b.ghost.set_visible(false);
                if let Some((f, _)) = b.cards.borrow().get(&gs.name) {
                    f.root.remove_css_class("lifted");
                    f.root.remove_css_class("resizing");
                }
                // resizing rebuilds the card: not from inside its own event
                let b2 = b.clone();
                glib::idle_add_local_once(move || match gs.target {
                    Some(t) => b2.commit(&gs.name, t),
                    None => b2.sync(),
                });
            });
        }
        self.fixed.add_controller(drag);

        // right click: sizes and collapse
        let menu = gtk::GestureClick::new();
        menu.set_button(gdk::BUTTON_SECONDARY);
        {
            let b = Rc::downgrade(self);
            menu.connect_pressed(move |g, _, x, y| {
                let Some(b) = b.upgrade() else { return };
                if let Some((name, _)) = b.hit(x, y) {
                    g.set_state(gtk::EventSequenceState::Claimed);
                    b.menu(&name);
                }
            });
        }
        self.fixed.add_controller(menu);
    }

    /// Saves `name` at `item`, along with where every other card is now, so nothing else moves.
    fn commit(&self, name: &str, item: Item) {
        let mut items = self.items();
        items.remove(name);
        let others: Vec<Item> = items.values().copied().collect();
        let grid = self.grid();
        // opening a collapsed card whose space was taken meanwhile: the closest spot that fits
        let Some(item) = (if grid.free(&item, &others) { Some(item) } else { grid.nearest_free(item, &others) }) else { return };
        items.insert(name.to_string(), item);
        self.ctx.edit_layout(|l| l.items = items);
    }

    fn menu(self: &Rc<Self>, name: &str) {
        let cards = self.cards.borrow();
        let Some((frame, _)) = cards.get(name) else { return };
        let items = self.items();
        let Some(item) = items.get(name).copied() else { return };
        let w = self.widget(name);

        let pop = gtk::Popover::new();
        pop.add_css_class("hw-menu");
        pop.set_has_arrow(false);
        let list = gtk::Box::new(gtk::Orientation::Vertical, 2);
        let title = gtk::Label::new(Some(w.title()));
        title.add_css_class("hw-menu-title");
        title.set_xalign(0.0);
        list.append(&title);
        let entry = |text: &str, on: bool| {
            let b = gtk::Button::new();
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            let check = gtk::Image::from_icon_name("object-select-symbolic");
            check.set_opacity(if on { 1.0 } else { 0.0 });
            row.append(&check);
            let l = gtk::Label::new(Some(text));
            l.set_xalign(0.0);
            row.append(&l);
            b.set_child(Some(&row));
            b.add_css_class("flat");
            b.add_css_class("hw-menu-item");
            list.append(&b);
            b
        };
        for &size in w.sizes() {
            let (cw, ch) = size.cells();
            let b = entry(&format!("{}  {cw}×{ch}", size.label()), size == item.size && !item.collapsed);
            let (board, name, pop) = (Rc::downgrade(self), name.to_string(), pop.clone());
            b.connect_clicked(move |_| {
                pop.popdown();
                let Some(board) = board.upgrade() else { return };
                let want = Item { size, collapsed: false, ..item };
                let others: Vec<Item> = board.items().into_iter().filter(|(n, _)| *n != name).map(|(_, i)| i).collect();
                let grid = board.grid();
                let spot = if grid.free(&want, &others) { Some(want) } else { grid.nearest_free(want, &others) };
                let name = name.clone();
                glib::idle_add_local_once(move || {
                    if let Some(t) = spot {
                        board.commit(&name, t);
                    }
                });
            });
        }
        if item.size != Size::Mini {
            let sep = gtk::Separator::new(gtk::Orientation::Horizontal);
            list.append(&sep);
            let b = entry(if item.collapsed { "Expand" } else { "Collapse" }, false);
            let (board, name, pop) = (Rc::downgrade(self), name.to_string(), pop.clone());
            b.connect_clicked(move |_| {
                pop.popdown();
                let Some(board) = board.upgrade() else { return };
                let name = name.clone();
                glib::idle_add_local_once(move || board.commit(&name, Item { collapsed: !item.collapsed, ..item }));
            });
        }
        pop.set_child(Some(&list));
        pop.set_parent(&frame.root);
        pop.connect_closed(|p| {
            let p = p.clone();
            glib::idle_add_local_once(move || p.unparent());
        });
        pop.popup();
    }
}
