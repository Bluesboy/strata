// SPDX-License-Identifier: MIT

//! Window state read from the widget tree by the names Strata's accessibility
//! helpers set, mirroring the Python harness's AT-SPI page object
//! (`tests/e2e/harness/browser.py`).

use std::{collections::HashMap, fmt::Write as _};

use gtk::{graphene, prelude::*};
use serde::Serialize;
use serde_json::{Value, json};

use super::protocol::{Check, Failure, Predicate};
use crate::ui::accessibility_qa::{self as names, Mark};

/// What the harness can see, as plain data so predicates are testable without GTK.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Observation {
    /// In left-to-right visual order.
    pub(crate) panes: Vec<PaneObservation>,
    pub(crate) focused_entry: Option<String>,
    pub(crate) focused_pane: Option<usize>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PaneObservation {
    pub(crate) name: String,
    pub(crate) view: &'static str,
    /// In visual order: top to bottom, then left to right.
    pub(crate) entries: Vec<EntryObservation>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct EntryObservation {
    pub(crate) name: String,
    pub(crate) selected: bool,
    pub(crate) focused: bool,
}

/// The `app` snapshot every reply carries.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct AppState {
    pub(crate) view_mode: Option<String>,
    pub(crate) directory: Option<String>,
    pub(crate) panes: Vec<PaneState>,
    pub(crate) focused: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct PaneState {
    pub(crate) name: String,
    pub(crate) selected: Vec<String>,
    pub(crate) focused: Option<String>,
}

impl Observation {
    /// Columns keeps parent panes on screen, so the pane holding keyboard focus
    /// is the current one; otherwise the deepest pane is.
    pub(crate) fn directory(&self) -> Option<&str> {
        self.focused_pane
            .and_then(|index| self.panes.get(index))
            .or_else(|| self.panes.last())
            .map(|pane| pane.name.as_str())
    }

    pub(crate) fn view_mode(&self) -> Option<&'static str> {
        self.panes.first().map(|pane| pane.view)
    }

    /// The named pane, or the deepest; the last match wins, as in the Python harness.
    pub(crate) fn pane(&self, directory: Option<&str>) -> Option<&PaneObservation> {
        match directory {
            None => self.panes.last(),
            Some(directory) => self.panes.iter().rev().find(|pane| pane.name == directory),
        }
    }

    pub(crate) fn entry_names(&self, directory: Option<&str>) -> Vec<String> {
        self.pane(directory).map_or_else(Vec::new, |pane| {
            pane.entries
                .iter()
                .map(|entry| entry.name.clone())
                .collect()
        })
    }

    /// Whether the named entry, in the named pane or the first pane listing
    /// it, is selected.
    pub(crate) fn entry_selected(&self, name: &str, directory: Option<&str>) -> bool {
        let panes: Vec<&PaneObservation> = match directory {
            Some(_) => self.pane(directory).into_iter().collect(),
            None => self.panes.iter().collect(),
        };
        panes
            .into_iter()
            .flat_map(|pane| &pane.entries)
            .find(|entry| entry.name == name)
            .is_some_and(|entry| entry.selected)
    }

    pub(crate) fn selected_names(&self, directory: Option<&str>) -> Vec<String> {
        self.pane(directory).map_or_else(Vec::new, |pane| {
            pane.entries
                .iter()
                .filter(|entry| entry.selected)
                .map(|entry| entry.name.clone())
                .collect()
        })
    }

    pub(crate) fn app_state(&self) -> AppState {
        AppState {
            view_mode: self.view_mode().map(str::to_owned),
            directory: self.directory().map(str::to_owned),
            panes: self
                .panes
                .iter()
                .map(|pane| PaneState {
                    name: pane.name.clone(),
                    selected: pane
                        .entries
                        .iter()
                        .filter(|entry| entry.selected)
                        .map(|entry| entry.name.clone())
                        .collect(),
                    focused: pane
                        .entries
                        .iter()
                        .find(|entry| entry.focused)
                        .map(|entry| entry.name.clone()),
                })
                .collect(),
            focused: self.focused_entry.clone(),
        }
    }

    pub(crate) fn evaluate(&self, predicate: &Predicate) -> Result<(), Failure> {
        let directory = predicate.dir.as_deref();
        let (holds, actual) = match &predicate.check {
            Check::Directory(expected) => {
                let actual = self.directory();
                (expected.as_deref() == actual, json!(actual))
            }
            Check::Panes(expected) => {
                let actual: Vec<&str> = self.panes.iter().map(|pane| pane.name.as_str()).collect();
                (*expected == actual, json!(actual))
            }
            Check::Entries(expected) => {
                let actual = self.entry_names(directory);
                (expected.matches(&actual), json!(actual))
            }
            Check::Selected(expected) => {
                let actual = self.selected_names(directory);
                (expected.matches(&actual), json!(actual))
            }
            Check::Focused(expected) => {
                (*expected == self.focused_entry, json!(self.focused_entry))
            }
            Check::ViewMode(expected) => {
                let actual = self.view_mode();
                (Some(expected.as_str()) == actual, json!(actual))
            }
        };
        if holds {
            return Ok(());
        }
        Err(Failure {
            predicate: serde_json::to_value(predicate).unwrap_or(Value::Null),
            actual,
            message: None,
        })
    }
}

/// An observation plus the widgets behind it, for aiming pointer input.
pub(crate) struct Scene {
    pub(crate) observation: Observation,
    entry_widgets: Vec<Vec<gtk::Widget>>,
}

impl Scene {
    pub(crate) fn entry_widget(&self, name: &str, directory: Option<&str>) -> Option<&gtk::Widget> {
        let panes = &self.observation.panes;
        let candidates: Vec<usize> = match directory {
            Some(directory) => panes
                .iter()
                .rposition(|pane| pane.name == directory)
                .into_iter()
                .collect(),
            None => (0..panes.len()).collect(),
        };
        candidates.into_iter().find_map(|pane| {
            let index = panes[pane]
                .entries
                .iter()
                .position(|entry| entry.name == name)?;
            self.entry_widgets[pane].get(index)
        })
    }
}

/// Bounds relative to the widget's own native surface, when the widget is
/// showing, visible, and finitely sized (AT-SPI's "rendered").
pub(crate) fn rendered_bounds(widget: &gtk::Widget) -> Option<graphene::Rect> {
    if !widget.is_mapped() || !widget.is_visible() {
        return None;
    }
    let native = widget.native()?;
    let bounds = widget.compute_bounds(&native)?;
    let sane = |extent: f32| extent > 0.0 && extent < 100_000.0;
    (sane(bounds.width()) && sane(bounds.height())).then_some(bounds)
}

fn descendants(root: &gtk::Widget) -> Vec<gtk::Widget> {
    let mut found = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(widget) = stack.pop() {
        let mut children = Vec::new();
        let mut child = widget.first_child();
        while let Some(current) = child {
            child = current.next_sibling();
            children.push(current);
        }
        stack.extend(children.into_iter().rev());
        found.push(widget);
    }
    found
}

fn address(widget: &gtk::Widget) -> usize {
    widget.as_ptr() as usize
}

struct Row {
    item: gtk::ListItem,
    name: String,
}

/// Entry names live on list items; the row widget carrying the item's focus
/// and selected states is the parent of the item's child.
fn rows() -> HashMap<usize, Row> {
    names::entries()
        .into_iter()
        .filter_map(|(item, naming)| {
            let row = item.child()?.parent()?;
            let name = naming.label.filter(|name| !name.is_empty())?;
            Some((address(&row), Row { item, name }))
        })
        .collect()
}

pub(crate) fn scan(window: &gtk::Window) -> Scene {
    let rows = rows();
    let focus = gtk::prelude::GtkWindowExt::focus(window).filter(WidgetExt::has_focus);
    let root = window.upcast_ref::<gtk::Widget>();
    let all = descendants(root);

    let mut panes: Vec<(f32, PaneObservation, Vec<gtk::Widget>, bool)> = Vec::new();
    for widget in &all {
        let Some(naming) = names::naming(widget) else {
            continue;
        };
        let Some(Mark::Pane(view)) = naming.mark else {
            continue;
        };
        let Some(bounds) = rendered_bounds(widget) else {
            continue;
        };
        let (entries, widgets) = pane_entries(widget, &rows);
        let holds_focus = focus
            .as_ref()
            .is_some_and(|focus| focus == widget || focus.is_ancestor(widget));
        panes.push((
            bounds.x(),
            PaneObservation {
                name: naming.label.unwrap_or_default(),
                view,
                entries,
            },
            widgets,
            holds_focus,
        ));
    }
    panes.sort_by(|left, right| left.0.total_cmp(&right.0));

    let focused_entry = all.iter().find_map(|widget| {
        let row = rows.get(&address(widget))?;
        (widget.has_focus() && rendered_bounds(widget).is_some()).then(|| row.name.clone())
    });
    let focused_pane = panes.iter().rposition(|pane| pane.3);
    let (observed, entry_widgets) = panes
        .into_iter()
        .map(|(_, pane, widgets, _)| (pane, widgets))
        .unzip();
    Scene {
        observation: Observation {
            panes: observed,
            focused_entry,
            focused_pane,
        },
        entry_widgets,
    }
}

fn pane_entries(
    pane: &gtk::Widget,
    rows: &HashMap<usize, Row>,
) -> (Vec<EntryObservation>, Vec<gtk::Widget>) {
    let container = descendants(pane).into_iter().find(|widget| {
        names::naming(widget).is_some_and(|naming| naming.mark == Some(Mark::EntryContainer))
            && rendered_bounds(widget).is_some()
    });
    let Some(container) = container else {
        return (Vec::new(), Vec::new());
    };
    let mut found: Vec<(graphene::Rect, EntryObservation, gtk::Widget)> = descendants(&container)
        .into_iter()
        .filter_map(|widget| {
            let row = rows.get(&address(&widget))?;
            let bounds = rendered_bounds(&widget)?;
            let entry = EntryObservation {
                name: row.name.clone(),
                selected: row.item.is_selected(),
                focused: widget.has_focus(),
            };
            Some((bounds, entry, widget))
        })
        .collect();
    found.sort_by(|left, right| {
        left.0
            .y()
            .total_cmp(&right.0.y())
            .then(left.0.x().total_cmp(&right.0.x()))
    });
    found
        .into_iter()
        .map(|(_, entry, widget)| (entry, widget))
        .unzip()
}

/// Whether the window can be driven: a pane with its entry list on screen,
/// as `tests/e2e/harness/application.py` waits for before a scenario starts.
pub(crate) fn is_ready(window: &gtk::Window) -> bool {
    window.is_mapped()
        && descendants(window.upcast_ref()).iter().any(|widget| {
            names::naming(widget).is_some_and(|naming| naming.mark == Some(Mark::EntryContainer))
                && rendered_bounds(widget).is_some()
        })
}

const MAX_TREE_DEPTH: usize = 64;

/// A readable dump of the showing widget tree, for timeouts and artifacts.
pub(crate) fn dump(window: &gtk::Window) -> String {
    let rows = rows();
    let mut output = String::new();
    dump_widget(window.upcast_ref(), 0, &rows, &mut output);
    output
}

fn dump_widget(
    widget: &gtk::Widget,
    depth: usize,
    rows: &HashMap<usize, Row>,
    output: &mut String,
) {
    if !widget.is_mapped() || depth > MAX_TREE_DEPTH {
        return;
    }
    let _ = write!(
        output,
        "{:indent$}{} role={:?}",
        "",
        widget.type_().name(),
        widget.accessible_role(),
        indent = depth * 2
    );
    if let Some(row) = rows.get(&address(widget)) {
        let _ = write!(output, " entry={:?}", row.name);
        if row.item.is_selected() {
            output.push_str(" [selected]");
        }
    }
    if let Some(naming) = names::naming(widget) {
        if let Some(label) = naming.label.as_deref() {
            let _ = write!(output, " label={label:?}");
        }
        if let Some(description) = naming
            .description
            .as_deref()
            .filter(|text| !text.is_empty())
        {
            let _ = write!(output, " description={description:?}");
        }
    }
    if let Some(text) = widget_text(widget) {
        let _ = write!(output, " text={text:?}");
    }
    if widget.has_focus() {
        output.push_str(" [focused]");
    }
    if let Some(bounds) = rendered_bounds(widget) {
        let _ = write!(
            output,
            " ({:.0},{:.0} {:.0}x{:.0})",
            bounds.x(),
            bounds.y(),
            bounds.width(),
            bounds.height()
        );
    }
    output.push('\n');
    let mut child = widget.first_child();
    while let Some(current) = child {
        dump_widget(&current, depth + 1, rows, output);
        child = current.next_sibling();
    }
}

fn widget_text(widget: &gtk::Widget) -> Option<String> {
    let text = if let Some(label) = widget.downcast_ref::<gtk::Label>() {
        label.label().to_string()
    } else if let Some(inscription) = widget.downcast_ref::<gtk::Inscription>() {
        inscription
            .text()
            .map(|text| text.to_string())
            .unwrap_or_default()
    } else {
        widget.downcast_ref::<gtk::Text>()?.text().to_string()
    };
    (!text.is_empty()).then_some(text)
}
