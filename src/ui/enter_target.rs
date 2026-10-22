// SPDX-License-Identifier: MIT

//! The filter result that Enter in the filter field opens.
//!
//! Until the user picks a result after the query last changed, Enter opens the top one.
//! While the field has focus, that row carries [`CSS_CLASS`], and the list keeps it
//! scrolled into view until the user scrolls it. The mark is not a selection: Space,
//! preview, and file actions still need one. The 10xer prompts keep their own two-step
//! Enter, so none of this applies there.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gtk::{glib, prelude::*};

pub(crate) const CSS_CLASS: &str = "enter-target";

#[derive(Debug, Default)]
struct Tracker {
    query: Option<String>,
    edited: bool,
    picked: bool,
    scrolled: bool,
}

impl Tracker {
    /// Records the query whose results are shown. A changed text, including one edited
    /// and typed back, drops the pick; returns whether it did.
    fn show_query(&mut self, query: &str, tenxer: bool) -> bool {
        let edited = std::mem::take(&mut self.edited);
        let query = query.trim();
        if query.is_empty() {
            self.clear();
            return false;
        }
        let changed = edited || self.query.as_deref() != Some(query);
        self.query = Some(query.to_owned());
        if !changed || tenxer {
            return false;
        }
        self.picked = false;
        self.scrolled = false;
        true
    }

    fn clear(&mut self) {
        *self = Self::default();
    }

    fn offers_top(&self, has_rows: bool, has_selection: bool, tenxer: bool) -> bool {
        !tenxer && self.query.is_some() && has_rows && !(self.picked && has_selection)
    }

    fn follows_top(&self, has_rows: bool, has_selection: bool, tenxer: bool) -> bool {
        !self.scrolled && self.offers_top(has_rows, has_selection, tenxer)
    }
}

#[derive(Default)]
struct Inner {
    tracker: RefCell<Tracker>,
    entry: glib::WeakRef<gtk::Entry>,
    view: glib::WeakRef<gtk::Widget>,
    selection: glib::WeakRef<gtk::MultiSelection>,
    rows: RefCell<Vec<(glib::WeakRef<gtk::ListItem>, glib::WeakRef<gtk::Widget>)>>,
    settle_queued: Cell<bool>,
    scroll_wanted: Cell<bool>,
}

/// One filter field and the result list Enter in it opens from.
#[derive(Clone, Default)]
pub(in crate::ui) struct EnterTarget(Rc<Inner>);

impl EnterTarget {
    pub(in crate::ui) fn new(entry: &gtk::Entry) -> Self {
        let target = Self::default();
        target.0.entry.set(Some(entry));
        let weak = Rc::downgrade(&target.0);
        entry.connect_changed(move |_| {
            if let Some(inner) = weak.upgrade() {
                inner.tracker.borrow_mut().edited = true;
            }
        });
        let focus = gtk::EventControllerFocus::new();
        let weak = Rc::downgrade(&target.0);
        focus.connect_enter(move |_| queue_settle(&weak, false));
        let weak = Rc::downgrade(&target.0);
        focus.connect_leave(move |_| queue_settle(&weak, false));
        entry.add_controller(focus);
        target
    }

    /// Follows the list whose top row Enter opens. Wheel or scrollbar use of `scroll`
    /// stops keeping the top in view until the query changes; focusing `view` picks.
    pub(in crate::ui) fn attach(
        &self,
        view: &impl IsA<gtk::Widget>,
        selection: &gtk::MultiSelection,
        scroll: &gtk::ScrolledWindow,
    ) {
        self.0.view.set(Some(view.upcast_ref()));
        self.0.selection.set(Some(selection));
        let weak = Rc::downgrade(&self.0);
        selection.connect_items_changed(move |_, _, _, _| queue_settle(&weak, true));
        let weak = Rc::downgrade(&self.0);
        selection.connect_selection_changed(move |_, _, _| queue_settle(&weak, false));

        let wheel = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
        wheel.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(&self.0);
        wheel.connect_scroll(move |_, _, _| {
            if let Some(inner) = weak.upgrade() {
                inner.tracker.borrow_mut().scrolled = true;
            }
            glib::Propagation::Proceed
        });
        scroll.add_controller(wheel);
        let drag = gtk::GestureClick::new();
        drag.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(&self.0);
        drag.connect_pressed(move |_, _, _, _| {
            if let Some(inner) = weak.upgrade() {
                inner.tracker.borrow_mut().scrolled = true;
            }
        });
        scroll.vscrollbar().add_controller(drag);

        let focus = gtk::EventControllerFocus::new();
        let weak = Rc::downgrade(&self.0);
        focus.connect_enter(move |_| {
            if let Some(inner) = weak.upgrade() {
                EnterTarget(inner).pick();
            }
        });
        view.add_controller(focus);
    }

    /// Records the query whose results the list now shows; true when the text changed,
    /// so the caller drops the results' previous selection.
    pub(in crate::ui) fn show_query(&self, query: &str) -> bool {
        let changed = self
            .0
            .tracker
            .borrow_mut()
            .show_query(query, super::tenxer_mode::chrome_suppressed());
        queue_settle(&Rc::downgrade(&self.0), true);
        changed
    }

    /// Treats the next shown query as new, after its previous results were dropped.
    pub(in crate::ui) fn restart(&self) {
        self.0.tracker.borrow_mut().query = None;
    }

    pub(in crate::ui) fn clear(&self) {
        self.0.tracker.borrow_mut().clear();
        self.refresh();
    }

    /// A selection or focus the user gave the results; Enter follows it from now on.
    pub(in crate::ui) fn pick(&self) {
        self.0.tracker.borrow_mut().picked = true;
        queue_settle(&Rc::downgrade(&self.0), false);
    }

    /// Whether Enter in the field opens the top result rather than the user's pick.
    pub(in crate::ui) fn offers_top(&self) -> bool {
        let Some((has_rows, has_selection)) = self.rows_state() else {
            return false;
        };
        self.0.tracker.borrow().offers_top(
            has_rows,
            has_selection,
            super::tenxer_mode::chrome_suppressed(),
        )
    }

    /// Marks the bound row when it is the one Enter opens; call from every bind.
    pub(in crate::ui) fn decorate(&self, widget: &impl IsA<gtk::Widget>, item: &gtk::ListItem) {
        let widget = widget.upcast_ref();
        let mut rows = self.0.rows.borrow_mut();
        rows.retain(|(item, row)| {
            item.upgrade().is_some() && row.upgrade().is_some_and(|row| &row != widget)
        });
        rows.push((item.downgrade(), widget.downgrade()));
        drop(rows);
        set_marked(widget, item.position() == 0 && self.shows_mark());
    }

    fn rows_state(&self) -> Option<(bool, bool)> {
        let selection = self.0.selection.upgrade()?;
        Some((selection.n_items() > 0, !selection.selection().is_empty()))
    }

    fn shows_mark(&self) -> bool {
        self.offers_top() && self.entry_has_focus()
    }

    fn entry_has_focus(&self) -> bool {
        let Some(entry) = self.0.entry.upgrade() else {
            return false;
        };
        entry
            .root()
            .and_then(|root| root.focus())
            .is_some_and(|focus| {
                focus == *entry.upcast_ref::<gtk::Widget>() || focus.is_ancestor(&entry)
            })
    }

    fn refresh(&self) {
        let shows = self.shows_mark();
        self.0.rows.borrow_mut().retain(|(item, widget)| {
            let (Some(item), Some(widget)) = (item.upgrade(), widget.upgrade()) else {
                return false;
            };
            set_marked(
                &widget,
                shows && item.item().is_some() && item.position() == 0,
            );
            true
        });
    }

    fn settle(&self) {
        self.refresh();
        if !self.0.scroll_wanted.replace(false) {
            return;
        }
        let Some((has_rows, has_selection)) = self.rows_state() else {
            return;
        };
        let follows = self.0.tracker.borrow().follows_top(
            has_rows,
            has_selection,
            super::tenxer_mode::chrome_suppressed(),
        );
        if follows && let Some(view) = self.0.view.upgrade() {
            super::browser::scroll_results_to(&view, 0, gtk::ListScrollFlags::NONE);
        }
    }
}

/// Coalesces bursts of model, selection, and focus changes into one idle refresh.
fn queue_settle(inner: &std::rc::Weak<Inner>, scroll: bool) {
    let Some(inner) = inner.upgrade() else {
        return;
    };
    if scroll {
        inner.scroll_wanted.set(true);
    }
    if inner.settle_queued.replace(true) {
        return;
    }
    let weak = Rc::downgrade(&inner);
    glib::idle_add_local_once(move || {
        if let Some(inner) = weak.upgrade() {
            inner.settle_queued.set(false);
            EnterTarget(inner).settle();
        }
    });
}

fn set_marked(widget: &gtk::Widget, marked: bool) {
    if marked {
        widget.add_css_class(CSS_CLASS);
    } else {
        widget.remove_css_class(CSS_CLASS);
    }
}

#[cfg(test)]
mod tests;
