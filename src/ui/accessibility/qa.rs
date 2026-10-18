// SPDX-License-Identifier: MIT

//! The `--qa` harness reads widgets back by the names these helpers give them.
//! GTK has no public getter for an accessible label, so the helpers also record
//! what they set here; without the `qa` feature this module does not exist.

use std::{cell::RefCell, collections::HashMap};

use gtk::{glib, prelude::*};

/// What a naming helper says a widget is, beyond its label and description.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mark {
    /// A browser pane, with the presentation name the harness reports.
    Pane(&'static str),
    EntryContainer,
    PaneSurface,
    /// A `gtk::ListItem`; the row widget is its child's parent.
    Entry,
    Dialog,
    MenuItem,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Naming {
    pub(crate) label: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) mark: Option<Mark>,
    /// The accessible `checked`, `pressed`, and `selected` states, which GTK
    /// cannot read back either.
    pub(crate) checked: Option<bool>,
    pub(crate) pressed: Option<bool>,
    pub(crate) selected: Option<bool>,
}

struct Record {
    object: glib::WeakRef<glib::Object>,
    naming: Naming,
    /// The widgets a `labelled-by` relation names, in order.
    labelled_by: Vec<glib::WeakRef<gtk::Widget>>,
}

const PRUNE_EVERY: usize = 1024;

#[derive(Default)]
struct Records {
    by_address: HashMap<usize, Record>,
    inserted: usize,
}

thread_local! {
    static RECORDS: RefCell<Records> = RefCell::default();
}

fn address(object: &glib::Object) -> usize {
    object.as_ptr() as usize
}

pub(crate) fn record(object: &impl ObjectType, update: impl FnOnce(&mut Naming)) {
    with_record(object, |record| update(&mut record.naming));
}

pub(crate) fn record_labelled_by(object: &impl ObjectType, labels: &[&gtk::Widget]) {
    with_record(object, |record| {
        record.labelled_by = labels.iter().map(|label| label.downgrade()).collect();
    });
}

fn with_record(object: &impl ObjectType, update: impl FnOnce(&mut Record)) {
    let Some(object) = object.dynamic_cast_ref::<glib::Object>() else {
        return;
    };
    RECORDS.with_borrow_mut(|records| {
        let key = address(object);
        // A freed object's address can be reused; its record must not leak into the new one.
        let stale = records
            .by_address
            .get(&key)
            .is_some_and(|record| record.object.upgrade().as_ref() != Some(object));
        if stale {
            records.by_address.remove(&key);
        }
        let record = records.by_address.entry(key).or_insert_with(|| Record {
            object: object.downgrade(),
            naming: Naming::default(),
            labelled_by: Vec::new(),
        });
        update(record);
        records.inserted += 1;
        if records.inserted.is_multiple_of(PRUNE_EVERY) {
            records
                .by_address
                .retain(|_, record| record.object.upgrade().is_some());
        }
    });
}

pub(crate) fn naming(object: &impl ObjectType) -> Option<Naming> {
    let object = object.dynamic_cast_ref::<glib::Object>()?;
    RECORDS.with_borrow(|records| {
        records
            .by_address
            .get(&address(object))
            .filter(|record| record.object.upgrade().as_ref() == Some(object))
            .map(|record| record.naming.clone())
    })
}

/// The widgets a recorded `labelled-by` relation names; empty without one.
pub(crate) fn labelled_by(object: &impl ObjectType) -> Vec<gtk::Widget> {
    let Some(object) = object.dynamic_cast_ref::<glib::Object>() else {
        return Vec::new();
    };
    RECORDS.with_borrow(|records| {
        records
            .by_address
            .get(&address(object))
            .filter(|record| record.object.upgrade().as_ref() == Some(object))
            .map(|record| {
                record
                    .labelled_by
                    .iter()
                    .filter_map(glib::WeakRef::upgrade)
                    .collect()
            })
            .unwrap_or_default()
    })
}

/// Every live list item an entry was described on, with the name it was given.
pub(crate) fn entries() -> Vec<(gtk::ListItem, Naming)> {
    RECORDS.with_borrow(|records| {
        records
            .by_address
            .values()
            .filter(|record| record.naming.mark == Some(Mark::Entry))
            .filter_map(|record| {
                let item = record.object.upgrade()?.downcast::<gtk::ListItem>().ok()?;
                Some((item, record.naming.clone()))
            })
            .collect()
    })
}
