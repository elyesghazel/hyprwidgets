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
    /// Behind an Rc so subscribers run with no borrow held: a subscriber may build widgets that
    /// bind() or read the value again.
    value: Option<Rc<T>>,
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
        let current = self.inner.borrow().value.clone();
        if let Some(v) = current {
            f(widget, &v);
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
        self.inner.borrow().value.as_deref().cloned()
    }

    pub fn set(&self, value: T) {
        // take the list out so a subscriber may call get() or bind() without a double borrow
        let value = Rc::new(value);
        let subscribers = {
            let mut inner = self.inner.borrow_mut();
            inner.value = Some(value.clone());
            std::mem::take(&mut inner.subscribers)
        };
        let mut kept = Vec::with_capacity(subscribers.len());
        for f in subscribers {
            // a subscriber that set() again made this value stale; it already told the rest
            if f(&value) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// A subscriber that subscribes others and reads the value (what a card rebuilt after a
    /// move does) must not panic.
    #[test]
    fn subscribers_can_bind_and_read() {
        let s: Shared<i32> = Shared::new();
        let owner = glib::Object::new::<glib::Object>();
        let inner_calls = Rc::new(Cell::new(0));
        {
            let (s2, owner2, calls) = (s.clone(), owner.clone(), inner_calls.clone());
            s.bind(&owner, move |_, v| {
                assert_eq!(s2.get(), Some(*v));
                let calls = calls.clone();
                s2.bind(&owner2, move |_, _| calls.set(calls.get() + 1));
            });
        }
        s.set(1);
        s.set(2);
        assert!(inner_calls.get() >= 2);
    }
}
