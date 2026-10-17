// SPDX-License-Identifier: MIT

//! Resolves a [`Target`] to a widget and a point on the X screen, the way the
//! Python page object (`tests/e2e/harness/browser.py`) finds a node by its
//! AT-SPI role and name and aims at its bounds.

use std::collections::HashMap;

use gtk::{glib, prelude::*};

use super::{
    input::Injector,
    protocol::{At, FieldRef, Target},
    state::{self, Scene},
};
use crate::ui::accessibility_qa as names;

/// Widgets a `remember` kept, by the name it gave them.
pub(crate) type Memory = HashMap<String, glib::WeakRef<gtk::Widget>>;

/// A box on the X screen, in root window coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Rect {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

impl Rect {
    pub(crate) fn centre(&self) -> (f64, f64) {
        (
            self.x + (self.width / 2.0).floor(),
            self.y + (self.height / 2.0).floor(),
        )
    }

    pub(crate) fn point(&self, at: Option<&At>) -> (f64, f64) {
        match at {
            None => self.centre(),
            Some(at) => (
                at.x.resolve(self.x, self.width),
                at.y.resolve(self.y, self.height),
            ),
        }
    }

    /// Whole pixels, as AT-SPI reports extents; sub-pixel drift is not movement.
    pub(crate) fn pixels(&self) -> [i64; 4] {
        [self.x, self.y, self.width, self.height].map(|value| value.floor() as i64)
    }
}

/// What a target resolved to: the widget, its box, and where a pointer event aims.
#[derive(Clone, Debug)]
pub(crate) struct Located {
    pub(crate) widget: gtk::Widget,
    pub(crate) rect: Rect,
    pub(crate) point: (f64, f64),
}

/// Why a lookup failed: the target is not there, which `absent` and the
/// patient lookups accept, or resolving it broke, which is always an error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LookupError {
    Missing(String),
    Broken(String),
}

impl LookupError {
    pub(crate) fn message(&self) -> &str {
        match self {
            Self::Missing(message) | Self::Broken(message) => message,
        }
    }
}

fn missing(message: impl Into<String>) -> LookupError {
    LookupError::Missing(message.into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Aim {
    /// The centre, or `at` from the corner.
    Point,
    /// `Pointer.drag_origin`: a file drag starts on the row's icon, not on
    /// the inert space in a wide row.
    DragOrigin,
}

/// Finds `target` now; callers poll, as the Python lookups wait.
pub(crate) fn locate(
    window: &gtk::Window,
    injector: &Injector,
    memory: &Memory,
    target: &Target,
    at: Option<&At>,
    aim: Aim,
) -> Result<Located, LookupError> {
    let scene = state::scan(window);
    let root = window.upcast_ref::<gtk::Widget>();
    let widget = match target {
        Target::Entry(entry) => {
            let row = scene
                .entry_widget(&entry.entry, entry.dir.as_deref())
                .cloned()
                .ok_or_else(|| {
                    missing(format!(
                        "no entry {:?}{} on screen",
                        entry.entry,
                        entry
                            .dir
                            .as_deref()
                            .map(|dir| format!(" in {dir:?}"))
                            .unwrap_or_default()
                    ))
                })?;
            if aim == Aim::DragOrigin {
                let icon = presented(&row).into_iter().find(|widget| {
                    role_name(widget) == "image" && state::rendered_bounds(widget).is_some()
                });
                let rect = screen_rect(icon.as_ref().unwrap_or(&row), injector)?;
                return Ok(Located {
                    widget: row,
                    rect,
                    point: rect.centre(),
                });
            }
            row
        }
        Target::Role { role, name } => find(root, role, name)?,
        Target::Sidebar(name) | Target::Header(name) => find(root, "button", name)?,
        Target::MenuItem(label) => {
            let menu = first_rendered(root, |widget| role_name(widget) == "menu");
            find(menu.as_ref().unwrap_or(root), "menu item", label)?
        }
        Target::DialogButton(label) => {
            let dialog = first_rendered(root, |widget| role_name(widget) == "dialog")
                .or_else(|| first_rendered(root, |widget| role_name(widget) == "alert"))
                .ok_or_else(|| missing("no dialog is open"))?;
            find(&dialog, "button", label)?
        }
        Target::Field(remembered) => field(
            window,
            memory,
            &FieldRef {
                of: remembered.clone(),
                name: None,
            },
        )?
        .ok_or_else(|| {
            missing(match remembered {
                None => "no editable field has focus".to_owned(),
                Some(name) => format!("the field remembered as {name:?} is gone"),
            })
        })?,
        Target::Result(path) => search_result(root, path)
            .ok_or_else(|| missing(format!("no result {path:?} on screen")))?,
        Target::Pane { dir } => {
            let pane = pane(&scene, dir.as_deref())?;
            let rect = screen_rect(&pane.pane, injector)?;
            // `empty_point`: inside the pane, below its last entry.
            let point = at.map_or(
                (
                    rect.x + (rect.width / 2.0).floor(),
                    rect.y + rect.height - 14.0,
                ),
                |at| rect.point(Some(at)),
            );
            return Ok(Located {
                widget: pane.pane.clone(),
                rect,
                point,
            });
        }
        Target::Background { dir } => return background(&scene, injector, dir.as_deref()),
        Target::Viewport { dir } => {
            let container = pane(&scene, dir.as_deref())?
                .container
                .clone()
                .ok_or_else(|| missing("the pane has no entry list"))?;
            std::iter::successors(container.parent(), WidgetExt::parent)
                .find(|widget| widget.is::<gtk::ScrolledWindow>())
                .ok_or_else(|| missing("the entry list has no scroll viewport"))?
        }
    };
    let rect = screen_rect(&widget, injector)?;
    Ok(Located {
        rect,
        point: rect.point(at),
        widget,
    })
}

fn pane<'a>(
    scene: &'a Scene,
    directory: Option<&str>,
) -> Result<&'a state::PaneWidgets, LookupError> {
    scene.pane_widgets(directory).ok_or_else(|| {
        missing(format!(
            "no {} pane on screen",
            directory.map_or_else(|| "browser".to_owned(), |dir| format!("{dir:?}"))
        ))
    })
}

/// `background_point`: on the entry list itself, below the last entry.
fn background(
    scene: &Scene,
    injector: &Injector,
    directory: Option<&str>,
) -> Result<Located, LookupError> {
    let widgets = pane(scene, directory)?;
    let Some(container) = widgets.container.as_ref() else {
        let rect = screen_rect(&widgets.pane, injector)?;
        return Ok(Located {
            widget: widgets.pane.clone(),
            rect,
            point: rect.centre(),
        });
    };
    let rect = screen_rect(container, injector)?;
    let mut lowest = rect.y;
    let mut any = false;
    for entry in &widgets.entries {
        let entry = screen_rect(entry, injector)?;
        let bottom = entry.y.floor() + entry.height.floor();
        lowest = if any { lowest.max(bottom) } else { bottom };
        any = true;
    }
    let y = (rect.y + rect.height - 6.0).min((lowest + 12.0).max(rect.y + 6.0));
    Ok(Located {
        widget: container.clone(),
        rect,
        point: (rect.x + (rect.width / 2.0).floor(), y),
    })
}

/// The first showing widget in DFS order with this role and name, as
/// `Node.find(role=..., name=...)` returns it.
fn find(root: &gtk::Widget, role: &str, name: &str) -> Result<gtk::Widget, LookupError> {
    first_rendered(root, |widget| {
        role_name(widget) == role && accessible_name(widget) == name
    })
    .ok_or_else(|| missing(format!("no {role} named {name:?} on screen")))
}

fn first_rendered(
    root: &gtk::Widget,
    matches: impl Fn(&gtk::Widget) -> bool,
) -> Option<gtk::Widget> {
    presented(root)
        .into_iter()
        .find(|widget| matches(widget) && state::rendered_bounds(widget).is_some())
}

/// `find(role=..., name=..., rendered=False)`: presented, but perhaps
/// scrolled away or not yet allocated.
pub(crate) fn find_presented(root: &gtk::Widget, role: &str, name: &str) -> Option<gtk::Widget> {
    presented(root)
        .into_iter()
        .find(|widget| role_name(widget) == role && accessible_name(widget) == name)
}

/// `dialog()`: the first dialog on screen, or else the first alert.
pub(crate) fn open_dialog(presented: &[gtk::Widget]) -> Option<gtk::Widget> {
    ["dialog", "alert"].into_iter().find_map(|role| {
        presented
            .iter()
            .find(|widget| role_name(widget) == role && state::rendered_bounds(widget).is_some())
            .cloned()
    })
}

/// The `Search results` list with its row role, or else the results table.
pub(crate) fn search_results(presented: &[gtk::Widget]) -> Option<(gtk::Widget, &'static str)> {
    [("list", "list item"), ("table", "table cell")]
        .into_iter()
        .find_map(|(role, row_role)| {
            presented
                .iter()
                .find(|widget| {
                    role_name(widget) == role
                        && state::rendered_bounds(widget).is_some()
                        && accessible_name(widget)
                            == crate::i18n::tr(crate::ui::SEARCH_RESULTS_LABEL)
                })
                .map(|results| (results.clone(), row_role))
        })
}

/// `result(strata, path)`: a result row named after the file whose path
/// label ends with `path`; without a results collection, any list item.
fn search_result(root: &gtk::Widget, path: &str) -> Option<gtk::Widget> {
    let everything = presented(root);
    let (container, row_role) =
        search_results(&everything).unwrap_or_else(|| (root.clone(), "list item"));
    let file_name = std::path::Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    presented(&container).into_iter().find(|row| {
        role_name(row) == row_role
            && state::rendered_bounds(row).is_some()
            && accessible_name(row) == file_name
            && presented(row).iter().any(|label| {
                role_name(label) == "label"
                    && state::rendered_bounds(label).is_some()
                    && accessible_name(label).ends_with(path)
            })
    })
}

/// A GTK editable has focus itself or, for an entry, through its inner text.
pub(crate) fn editable_focused(widget: &gtk::Widget) -> bool {
    let Some(focus) = widget
        .root()
        .and_then(|root| root.focus())
        .filter(WidgetExt::has_focus)
    else {
        return false;
    };
    focus == *widget
        || widget
            .dynamic_cast_ref::<gtk::Editable>()
            .and_then(EditableExt::delegate)
            .is_some_and(|delegate| delegate.upcast::<gtk::Widget>() == focus)
}

fn is_field(widget: &gtk::Widget) -> bool {
    role_name(widget) == "text"
        && widget
            .dynamic_cast_ref::<gtk::Editable>()
            .is_some_and(EditableExt::is_editable)
}

/// `editable_field()`: the first editable text on screen holding focus.
pub(crate) fn focused_field(presented: &[gtk::Widget]) -> Option<gtk::Widget> {
    presented
        .iter()
        .find(|widget| {
            is_field(widget) && editable_focused(widget) && state::rendered_bounds(widget).is_some()
        })
        .cloned()
}

/// The field a check reads: one kept by `remember` (shown or not, as a kept
/// node still answers), the first on screen with the name, or the focused one.
/// A name nothing was remembered under is a broken scenario.
pub(crate) fn field(
    window: &gtk::Window,
    memory: &Memory,
    field: &FieldRef,
) -> Result<Option<gtk::Widget>, LookupError> {
    if let Some(remembered) = &field.of {
        return memory
            .get(remembered)
            .map(glib::WeakRef::upgrade)
            .ok_or_else(|| {
                LookupError::Broken(format!("no field was remembered as {remembered:?}"))
            });
    }
    let everything = presented(window.upcast_ref());
    Ok(match &field.name {
        Some(name) => everything.into_iter().find(|widget| {
            is_field(widget)
                && state::rendered_bounds(widget).is_some()
                && accessible_name(widget) == *name
        }),
        None => focused_field(&everything),
    })
}

/// A widget's AT-SPI states, as far as a scenario asks about them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct States {
    pub(crate) selected: bool,
    pub(crate) focused: bool,
    pub(crate) checked: bool,
    pub(crate) pressed: bool,
    pub(crate) sensitive: bool,
}

pub(crate) fn states(widget: &gtk::Widget) -> States {
    // Native toggles report their own state, so a recorded one cannot go stale.
    let recorded = names::naming(widget);
    let checked = if let Some(check) = widget.downcast_ref::<gtk::CheckButton>() {
        check.is_active()
    } else if let Some(switch) = widget.downcast_ref::<gtk::Switch>() {
        switch.is_active()
    } else {
        recorded
            .as_ref()
            .and_then(|naming| naming.checked)
            .unwrap_or(false)
    };
    let pressed = if let Some(toggle) = widget.downcast_ref::<gtk::ToggleButton>() {
        toggle.is_active()
    } else {
        recorded.and_then(|naming| naming.pressed).unwrap_or(false)
    };
    States {
        selected: state::row_selected(widget),
        focused: if widget.is::<gtk::Editable>() {
            editable_focused(widget)
        } else {
            widget.has_focus()
        },
        checked,
        pressed,
        sensitive: widget.is_sensitive(),
    }
}

/// Text a widget shows, as AT-SPI's text interface reads it.
pub(crate) fn text_content(widget: &gtk::Widget) -> Option<String> {
    if let Some(label) = widget.downcast_ref::<gtk::Label>() {
        return Some(label.text().to_string());
    }
    if let Some(inscription) = widget.downcast_ref::<gtk::Inscription>() {
        return inscription.text().map(|text| text.to_string());
    }
    if let Some(view) = widget.downcast_ref::<gtk::TextView>() {
        let buffer = view.buffer();
        let (start, end) = buffer.bounds();
        return Some(buffer.text(&start, &end, false).to_string());
    }
    widget
        .dynamic_cast_ref::<gtk::Editable>()
        .map(|editable| editable.text().to_string())
}

/// The bounds a widget is drawn at, on the X screen: its box in its native
/// surface, the surface's own content offset, and the surface's screen
/// origin. Popovers are their own native surfaces, so this covers them too.
pub(crate) fn screen_rect(widget: &gtk::Widget, injector: &Injector) -> Result<Rect, LookupError> {
    let broken = |message: &str| LookupError::Broken(message.to_owned());
    let bounds =
        state::rendered_bounds(widget).ok_or_else(|| missing("the target is not on screen"))?;
    let native = widget
        .native()
        .ok_or_else(|| broken("the target has no native surface"))?;
    let surface = native
        .surface()
        .ok_or_else(|| broken("the target's surface is not realized"))?;
    let (offset_x, offset_y) = native.surface_transform();
    let (origin_x, origin_y) = injector
        .surface_origin(&surface)
        .map_err(LookupError::Broken)?;
    Ok(Rect {
        x: f64::from(origin_x) + offset_x + f64::from(bounds.x()),
        y: f64::from(origin_y) + offset_y + f64::from(bounds.y()),
        width: f64::from(bounds.width()),
        height: f64::from(bounds.height()),
    })
}

/// The widgets AT-SPI presents under `root`, `root` first, in DFS order:
/// hidden widgets and everything inside them are left out.
pub(crate) fn presented(root: &gtk::Widget) -> Vec<gtk::Widget> {
    let mut found = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(widget) = stack.pop() {
        if !widget.is_visible() {
            continue;
        }
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

/// The AT-SPI role name the Python harness matches, with `push button`
/// spelled `button` as `tree.Node.role` does.
pub(crate) fn role_name(widget: &gtk::Widget) -> &'static str {
    use gtk::AccessibleRole as Role;
    if widget.is::<gtk::ScrolledWindow>() {
        return "scroll pane";
    }
    match widget.accessible_role() {
        Role::AlertDialog => "alert",
        Role::Alert => "notification",
        Role::Button => "button",
        Role::Caption => "caption",
        Role::Cell | Role::GridCell => "table cell",
        Role::Checkbox => "check box",
        Role::ColumnHeader => "column header",
        Role::ComboBox => "combo box",
        Role::Dialog => "dialog",
        Role::Grid | Role::Table => "table",
        Role::Group | Role::TabPanel => "panel",
        Role::Heading => "heading",
        Role::Img => "image",
        Role::Input | Role::SearchBox => "entry",
        Role::Label => "label",
        Role::Link => "link",
        Role::List => "list",
        Role::ListBox => "list box",
        Role::ListItem => "list item",
        Role::Menu => "menu",
        Role::MenuBar => "menu bar",
        Role::MenuItem => "menu item",
        Role::MenuItemCheckbox => "check menu item",
        Role::MenuItemRadio => "radio menu item",
        Role::Meter => "level bar",
        Role::ProgressBar => "progress bar",
        Role::Radio => "radio button",
        Role::Row => "table row",
        Role::RowHeader => "row header",
        Role::Scrollbar => "scroll bar",
        Role::Separator => "separator",
        Role::Slider => "slider",
        Role::SpinButton => "spin button",
        Role::Status => "status bar",
        Role::Switch | Role::ToggleButton => "toggle button",
        Role::Tab => "page tab",
        Role::TabList => "page tab list",
        Role::TextBox => "text",
        Role::Toolbar => "tool bar",
        Role::Tooltip => "tool tip",
        Role::Tree => "tree",
        Role::TreeGrid => "tree table",
        Role::TreeItem => "tree item",
        Role::Window => "frame",
        _ => "filler",
    }
}

/// Roles whose accessible name GTK builds from their children's names.
fn named_from_content(widget: &gtk::Widget) -> bool {
    use gtk::AccessibleRole as Role;
    matches!(
        widget.accessible_role(),
        Role::Button
            | Role::Cell
            | Role::Checkbox
            | Role::ColumnHeader
            | Role::GridCell
            | Role::Heading
            | Role::Link
            | Role::MenuItem
            | Role::MenuItemCheckbox
            | Role::MenuItemRadio
            | Role::Option
            | Role::Radio
            | Role::Row
            | Role::RowHeader
            | Role::Switch
            | Role::Tab
            | Role::ToggleButton
            | Role::Tooltip
            | Role::TreeItem
    )
}

/// The accessible name as AT-SPI reports it: the label Strata's naming
/// helpers set (on the list item, for an entry row), a label's own text, or
/// the children's names for roles named from content. Tooltips never name a
/// widget here; a control that needs a name gets an explicit label.
pub(crate) fn accessible_name(widget: &gtk::Widget) -> String {
    let mut naming = Vec::new();
    if let Some(name) = own_name(widget, &mut naming) {
        return name;
    }
    if named_from_content(widget) {
        return content_name(widget, &mut naming);
    }
    String::new()
}

/// `naming` holds the widgets whose labelled-by relation is being followed,
/// so a relation that leads back to one of them cannot recurse forever.
fn own_name(widget: &gtk::Widget, naming: &mut Vec<gtk::Widget>) -> Option<String> {
    if let Some(label) = names::naming(widget)
        .and_then(|naming| naming.label)
        .filter(|label| !label.is_empty())
    {
        return Some(label);
    }
    let labels = names::labelled_by(widget);
    if !labels.is_empty() && !naming.contains(widget) {
        naming.push(widget.clone());
        let mut parts = Vec::new();
        for label in &labels {
            let part = if label == widget {
                content_name(widget, naming)
            } else if naming.contains(label) {
                String::new()
            } else {
                own_name(label, naming).unwrap_or_else(|| content_name(label, naming))
            };
            if !part.is_empty() {
                parts.push(part);
            }
        }
        naming.pop();
        return Some(parts.join(" "));
    }
    // GtkMenuButton labels its inner toggle button with its own name.
    if widget.is::<gtk::ToggleButton>()
        && let Some(menu_button) = widget.parent().and_downcast::<gtk::MenuButton>()
    {
        return own_name(menu_button.upcast_ref(), naming);
    }
    if matches!(
        widget.accessible_role(),
        gtk::AccessibleRole::ListItem | gtk::AccessibleRole::GridCell | gtk::AccessibleRole::Cell
    ) && let Some(name) = state::row_name(widget)
    {
        return Some(name);
    }
    if let Some(label) = widget.downcast_ref::<gtk::Label>() {
        return Some(label.text().to_string());
    }
    widget
        .downcast_ref::<gtk::Inscription>()
        .map(|inscription| {
            inscription
                .text()
                .map(|text| text.to_string())
                .unwrap_or_default()
        })
}

fn content_name(widget: &gtk::Widget, naming: &mut Vec<gtk::Widget>) -> String {
    let mut parts = Vec::new();
    let mut child = widget.first_child();
    while let Some(current) = child {
        child = current.next_sibling();
        // A popover is its own surface, not part of the button that opens it.
        if !current.is_visible() || current.is::<gtk::Native>() {
            continue;
        }
        let part = own_name(&current, naming).unwrap_or_else(|| content_name(&current, naming));
        if !part.is_empty() {
            parts.push(part);
        }
    }
    parts.join(" ")
}
