//! A value that many widgets show at once (one copy per monitor). Background threads produce it,
//! the GTK main loop stores it and tells every subscriber.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::glib;
use gtk4::prelude::*;

pub struct Shared<T> {
    inner: Rc<RefCell<Inner<T>>>,
}

struct Inner<T> {
    value: Option<T>,
    subscribers: Vec<Box<dyn Fn(&T) -> bool>>,
}

impl<T> Clone for Shared<T> {
    fn clone(&self) -> Self {
        Shared { inner: self.inner.clone() }
    }
}

impl<T: 'static> Shared<T> {
    pub fn new() -> Self {
        Shared { inner: Rc::new(RefCell::new(Inner { value: None, subscribers: Vec::new() })) }
    }

    /// Calls `f(widget, value)` now if there is a value and on every later update, for as long as
    /// `widget` is alive. Widgets are rebuilt when monitors come and go, so only a weak ref is kept.
    pub fn bind<W: IsA<glib::Object>>(&self, widget: &W, f: impl Fn(&W, &T) + 'static) {
        if let Some(v) = &self.inner.borrow().value {
            f(widget, v);
        }
        let weak = widget.downgrade();
        self.inner.borrow_mut().subscribers.push(Box::new(move |v| match weak.upgrade() {
            Some(w) => {
                f(&w, v);
                true
            }
            None => false,
        }));
    }

    pub fn get(&self) -> Option<T>
    where
        T: Clone,
    {
        self.inner.borrow().value.clone()
    }

    pub fn set(&self, value: T) {
        // take the list out so a subscriber may call get() or bind() without a double borrow
        let subscribers = {
            let mut inner = self.inner.borrow_mut();
            inner.value = Some(value);
            std::mem::take(&mut inner.subscribers)
        };
        let mut kept = Vec::with_capacity(subscribers.len());
        for f in subscribers {
            let alive = {
                let inner = self.inner.borrow();
                f(inner.value.as_ref().unwrap())
            };
            if alive {
                kept.push(f);
            }
        }
        let mut inner = self.inner.borrow_mut();
        kept.append(&mut inner.subscribers);
        inner.subscribers = kept;
    }

    /// Feeds every message from `rx` into this value on the main loop.
    pub fn feed(&self, rx: async_channel::Receiver<T>) {
        let this = self.clone();
        glib::spawn_future_local(async move {
            while let Ok(v) = rx.recv().await {
                this.set(v);
            }
        });
    }
}
