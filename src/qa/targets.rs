// SPDX-License-Identifier: MIT

//! Mirrors how the Python page object (`tests/e2e/harness/browser.py`) finds AT-SPI
//! nodes by role and name and aims at their bounds.

use std::collections::HashMap;

use gtk::{glib, prelude::*};

use super::{
    input::Injector,
    protocol::{At, Axis, EntryPart, FieldRef, StateName, Target},
    state::{self, Scene},
};
use crate::ui::accessibility_qa as names;

pub(crate) type Memory = HashMap<String, glib::WeakRef<gtk::Widget>>;

pub(crate) type Boxes = HashMap<String, Rect>;

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

#[derive(Clone, Debug)]
pub(crate) struct Located {
    pub(crate) widget: gtk::Widget,
    pub(crate) rect: Rect,
    pub(crate) point: (f64, f64),
}

/// `Missing` is accepted by `absent` and the patient lookups; `Broken` is always an error.
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
    Point,
    DragOrigin,
}

#[derive(Clone, Copy)]
pub(crate) struct Kept<'a> {
    pub(crate) memory: &'a Memory,
    pub(crate) widgets: &'a Memory,
    pub(crate) boxes: &'a Boxes,
}

fn icon(widget: &gtk::Widget) -> Option<gtk::Widget> {
    presented(widget)
        .into_iter()
        .find(|widget| role_name(widget) == "image" && state::rendered_bounds(widget).is_some())
}

pub(crate) fn locate(
    window: &gtk::Window,
    injector: &Injector,
    kept: Kept<'_>,
    target: &Target,
    at: Option<&At>,
    aim: Aim,
) -> Result<Located, LookupError> {
    let memory = kept.memory;
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
            match entry.part {
                Some(EntryPart::Label) => first_rendered(&row, |widget| {
                    role_name(widget) == "label" && accessible_name(widget) == entry.entry
                })
                .ok_or_else(|| missing(format!("no name label on {:?}", entry.entry)))?,
                Some(EntryPart::Icon) => {
                    icon(&row).ok_or_else(|| missing(format!("no icon on {:?}", entry.entry)))?
                }
                Some(EntryPart::Metadata) => first_rendered(&row, |widget| {
                    role_name(widget) == "label" && {
                        let name = accessible_name(widget);
                        !name.is_empty() && name != entry.entry
                    }
                })
                .ok_or_else(|| missing(format!("no metadata label on {:?}", entry.entry)))?,
                None if aim == Aim::DragOrigin => return drag_origin(row, injector),
                None => row,
            }
        }
        Target::SearchResult(name) => {
            let everything = presented(root);
            let result = match search_results(&everything) {
                Some((results, row_role)) => find(&results, row_role, name),
                None => find(root, "list item", name),
            }?;
            if aim == Aim::DragOrigin {
                return drag_origin(result, injector);
            }
            result
        }
        Target::Role {
            within, rendered, ..
        } => {
            let wanted = Wanted::of(target)?;
            let scope = match within {
                None => root.clone(),
                Some(within) => locate(window, injector, kept, within, None, Aim::Point)?.widget,
            };
            if *rendered {
                wanted.find(&scope)?
            } else {
                wanted
                    .find_presented(&scope)
                    .ok_or_else(|| missing(format!("no {} presented", wanted.role)))?
            }
        }
        Target::Row(name) => ["list item", "table cell"]
            .into_iter()
            .find_map(|role| find(root, role, name).ok())
            .ok_or_else(|| missing(format!("no row named {name:?} on screen")))?,
        Target::FieldWithin(within) => {
            let scope = locate(window, injector, kept, within, None, Aim::Point)?.widget;
            focused_field(&presented(&scope))
                .ok_or_else(|| missing("no editable field inside the target has focus"))?
        }
        Target::Toplevel(title) => toplevel(title)
            .ok_or_else(|| missing(format!("no window titled {title:?} on screen")))?,
        Target::OtherWindow(title) => other_window(window, title)
            .ok_or_else(|| missing(format!("no other window titled {title:?} on screen")))?,
        Target::Named { name, within } => {
            let scope = locate(window, injector, kept, within, None, Aim::Point)?.widget;
            first_rendered(&scope, |widget| accessible_name(widget) == *name)
                .ok_or_else(|| missing(format!("nothing named {name:?} on screen")))?
        }
        Target::Container { dir } => pane(&scene, dir.as_deref())?
            .container
            .clone()
            .ok_or_else(|| missing("the pane has no entry list"))?,
        Target::Dialog => {
            open_dialog(&presented(root)).ok_or_else(|| missing("no dialog is open"))?
        }
        Target::Parent(of) => locate(window, injector, kept, of, None, Aim::Point)?
            .widget
            .parent()
            .ok_or_else(|| missing("the target has no parent"))?,
        Target::Ancestor { role, of } => {
            let child = locate(window, injector, kept, of, None, Aim::Point)?.widget;
            std::iter::successors(child.parent(), WidgetExt::parent)
                .find(|widget| role_name(widget) == role)
                .ok_or_else(|| missing(format!("the target has no {role} around it")))?
        }
        Target::Kept(name) => {
            let rect = kept.boxes.get(name).copied().ok_or_else(|| {
                LookupError::Broken(format!("no box or point was kept as {name:?}"))
            })?;
            return Ok(Located {
                widget: root.clone(),
                rect,
                point: rect.point(at),
            });
        }
        Target::Point(axes) => {
            let [x, y] = &**axes;
            let rect = Rect {
                x: coordinate(window, injector, kept, x, Along::X)? as f64,
                y: coordinate(window, injector, kept, y, Along::Y)? as f64,
                width: 0.0,
                height: 0.0,
            };
            return Ok(Located {
                widget: root.clone(),
                rect,
                point: rect.point(at),
            });
        }
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
        Target::ResultRow {
            index,
            containing,
            name: named,
        } => {
            let rows: Vec<gtk::Widget> = result_rows(root)
                .into_iter()
                .filter(|row| {
                    let name = accessible_name(row);
                    containing.as_deref().is_none_or(|part| name.contains(part))
                        && named.as_deref().is_none_or(|named| name == named)
                })
                .collect();
            pick(rows, *index).ok_or_else(|| missing(format!("no result row {index}")))?
        }
        Target::FocusedWidget => presented(root)
            .into_iter()
            .find(|widget| states(widget).focused && state::rendered_bounds(widget).is_some())
            .ok_or_else(|| missing("no widget has focus"))?,
        Target::Remembered(name) => kept
            .widgets
            .get(name)
            .ok_or_else(|| LookupError::Broken(format!("no widget was remembered as {name:?}")))?
            .upgrade()
            .ok_or_else(|| missing(format!("the widget remembered as {name:?} is gone")))?,
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
        Target::Window { y_of } => {
            let rect = screen_rect(root, injector)?;
            let (x, mut y) = rect.point(at);
            if let Some(other) = y_of {
                y = locate(window, injector, kept, other, None, Aim::Point)?
                    .rect
                    .centre()
                    .1;
            }
            return Ok(Located {
                widget: root.clone(),
                rect,
                point: (x, y),
            });
        }
        Target::Preview => {
            let label = crate::i18n::tr(crate::ui::PREVIEW_LABEL);
            first_rendered(root, |widget| accessible_name(widget) == label)
                .ok_or_else(|| missing("the preview is not open"))?
        }
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
    if aim == Aim::DragOrigin {
        return drag_origin(widget, injector);
    }
    let rect = screen_rect(&widget, injector)?;
    Ok(Located {
        rect,
        point: rect.point(at),
        widget,
    })
}

/// A file drag starts on the row's icon, not on the inert space in a wide row.
fn drag_origin(row: gtk::Widget, injector: &Injector) -> Result<Located, LookupError> {
    let rect = screen_rect(icon(&row).as_ref().unwrap_or(&row), injector)?;
    Ok(Located {
        widget: row,
        rect,
        point: rect.centre(),
    })
}

#[derive(Clone, Copy)]
enum Along {
    X,
    Y,
}

/// Whole pixels, as AT-SPI reports a box, so the arithmetic is the twin's integer arithmetic.
fn coordinate(
    window: &gtk::Window,
    injector: &Injector,
    kept: Kept<'_>,
    axis: &Axis,
    along: Along,
) -> Result<i64, LookupError> {
    Ok(match axis {
        Axis::Of { target, at, plus } => {
            let [x, y, width, height] = locate(window, injector, kept, target, None, Aim::Point)?
                .rect
                .pixels();
            let (start, extent) = match along {
                Along::X => (x, width),
                Along::Y => (y, height),
            };
            let value = match at {
                None => start + extent.div_euclid(2),
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "an offset from whole pixels is a whole pixel"
                )]
                Some(at) => at.resolve(start as f64, extent as f64).floor() as i64,
            };
            value + i64::from(*plus)
        }
        Axis::Mid { pair, plus } => {
            let [first, second] = &**pair;
            (coordinate(window, injector, kept, first, along)?
                + coordinate(window, injector, kept, second, along)?)
            .div_euclid(2)
                + i64::from(*plus)
        }
        Axis::Extent { target, plus } => {
            let [_, _, width, height] = locate(window, injector, kept, target, None, Aim::Point)?
                .rect
                .pixels();
            let extent = match along {
                Along::X => width,
                Along::Y => height,
            };
            extent + i64::from(*plus)
        }
        Axis::Max { pair, plus } => {
            let [first, second] = &**pair;
            coordinate(window, injector, kept, first, along)?
                .max(coordinate(window, injector, kept, second, along)?)
                + i64::from(*plus)
        }
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

pub(crate) struct Wanted<'a> {
    pub(crate) role: &'a str,
    pub(crate) other_roles: &'a [String],
    pub(crate) name: Option<&'a str>,
    pub(crate) description: Option<&'a str>,
    pub(crate) name_ends_with: &'a [String],
    pub(crate) nth: Option<i64>,
    pub(crate) states: &'a [StateName],
    pub(crate) name_matches: Option<regex::Regex>,
    pub(crate) has: Option<Box<Wanted<'a>>>,
}

pub(crate) fn pattern(pattern: Option<&str>) -> Result<Option<regex::Regex>, LookupError> {
    pattern
        .map(regex::Regex::new)
        .transpose()
        .map_err(|error| LookupError::Broken(error.to_string()))
}

pub(crate) fn has_state(widget: &gtk::Widget, state: StateName) -> bool {
    let now = states(widget);
    let orientation = widget
        .dynamic_cast_ref::<gtk::Orientable>()
        .map(OrientableExt::orientation);
    match state {
        StateName::Selected => now.selected,
        StateName::Focused => now.focused,
        StateName::Checked => now.checked,
        StateName::Pressed => now.pressed,
        StateName::Sensitive => now.sensitive,
        StateName::Selectable => now.selectable,
        StateName::Editable => now.editable,
        StateName::Horizontal => orientation == Some(gtk::Orientation::Horizontal),
        StateName::Vertical => orientation == Some(gtk::Orientation::Vertical),
    }
}

impl<'a> Wanted<'a> {
    pub(crate) fn of(target: &'a Target) -> Result<Self, LookupError> {
        let Target::Role {
            role,
            other_roles,
            name,
            description,
            name_ends_with,
            nth,
            states,
            name_matches,
            has,
            ..
        } = target
        else {
            return Err(LookupError::Broken(format!(
                "{} is not a role target",
                target.to_json()
            )));
        };
        Ok(Self {
            role,
            other_roles,
            name: name.as_deref(),
            description: description.as_deref(),
            name_ends_with,
            nth: *nth,
            states,
            name_matches: pattern(name_matches.as_deref())?,
            has: has
                .as_deref()
                .map(|has| Self::of(has).map(Box::new))
                .transpose()?,
        })
    }

    pub(crate) fn matches(&self, widget: &gtk::Widget) -> bool {
        let role = role_name(widget);
        (self.role == "*"
            || role == self.role
            || self.other_roles.iter().any(|other| other == role))
            && self.has.as_ref().is_none_or(|has| {
                presented(widget)
                    .iter()
                    .any(|inner| has.matches(inner) && state::rendered_bounds(inner).is_some())
            })
            && self.name.is_none_or(|name| accessible_name(widget) == name)
            && self
                .name_matches
                .as_ref()
                .is_none_or(|pattern| pattern.is_match(&accessible_name(widget)))
            && self.states.iter().all(|state| has_state(widget, *state))
            && (self.name_ends_with.is_empty() || {
                let name = accessible_name(widget);
                self.name_ends_with
                    .iter()
                    .any(|suffix| name.ends_with(suffix.as_str()))
            })
            && self
                .description
                .is_none_or(|description| accessible_description(widget) == description)
    }

    pub(crate) fn find(&self, root: &gtk::Widget) -> Result<gtk::Widget, LookupError> {
        let found = presented(root)
            .into_iter()
            .filter(|widget| self.matches(widget) && state::rendered_bounds(widget).is_some());
        pick(found.collect(), self.nth.unwrap_or(0)).ok_or_else(|| {
            missing(format!(
                "no {}{}{}{}{} on screen",
                self.nth
                    .map(|nth| format!("match {nth} of "))
                    .unwrap_or_default(),
                self.role,
                self.name
                    .map(|name| format!(" named {name:?}"))
                    .unwrap_or_default(),
                if self.name_ends_with.is_empty() {
                    String::new()
                } else {
                    format!(" named with an ending in {:?}", self.name_ends_with)
                },
                self.description
                    .map(|description| format!(" described {description:?}"))
                    .unwrap_or_default()
            ))
        })
    }

    pub(crate) fn find_presented(&self, root: &gtk::Widget) -> Option<gtk::Widget> {
        let found = presented(root)
            .into_iter()
            .filter(|widget| self.matches(widget));
        pick(found.collect(), self.nth.unwrap_or(0))
    }
}

fn pick(mut items: Vec<gtk::Widget>, index: i64) -> Option<gtk::Widget> {
    let length = i64::try_from(items.len()).ok()?;
    let index = if index < 0 { length + index } else { index };
    let index = usize::try_from(index).ok()?;
    (index < items.len()).then(|| items.swap_remove(index))
}

pub(crate) fn result_rows(root: &gtk::Widget) -> Vec<gtk::Widget> {
    let everything = presented(root);
    let (container, row_role) =
        search_results(&everything).unwrap_or_else(|| (root.clone(), "list item"));
    presented(&container)
        .into_iter()
        .filter(|row| role_name(row) == row_role && state::rendered_bounds(row).is_some())
        .collect()
}

pub(crate) fn windows_titled(title: &str) -> Vec<gtk::Window> {
    gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|window| window.downcast::<gtk::Window>().ok())
        .filter(|window| {
            window.title().as_deref() == Some(title)
                && state::rendered_bounds(window.upcast_ref()).is_some()
        })
        .collect()
}

pub(crate) fn window_titled(title: &str, but: Option<&gtk::Window>) -> Option<gtk::Window> {
    windows_titled(title)
        .into_iter()
        .find(|window| Some(window) != but)
}

fn toplevel(title: &str) -> Option<gtk::Widget> {
    gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|window| window.downcast::<gtk::Window>().ok())
        .find(|window| {
            window.title().as_deref() == Some(title)
                && state::rendered_bounds(window.upcast_ref()).is_some()
        })
        .map(Cast::upcast)
}

fn other_window(main: &gtk::Window, title: &str) -> Option<gtk::Widget> {
    gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|window| window.downcast::<gtk::Window>().ok())
        .find(|window| {
            window != main
                && window.title().as_deref() == Some(title)
                && state::rendered_bounds(window.upcast_ref()).is_some()
        })
        .map(Cast::upcast)
}

pub(crate) fn presented_target(
    window: &gtk::Window,
    injector: &Injector,
    kept: Kept<'_>,
    target: &Target,
) -> Result<Option<gtk::Widget>, LookupError> {
    match target {
        Target::Role { within, .. } => {
            let wanted = Wanted::of(target)?;
            let scope = match within {
                None => window.clone().upcast::<gtk::Widget>(),
                Some(within) => match locate(window, injector, kept, within, None, Aim::Point) {
                    Ok(located) => located.widget,
                    Err(LookupError::Missing(_)) => return Ok(None),
                    Err(broken) => return Err(broken),
                },
            };
            Ok(wanted.find_presented(&scope))
        }
        other => match locate(window, injector, kept, other, None, Aim::Point) {
            Ok(located) => Ok(Some(located.widget)),
            Err(LookupError::Missing(_)) => Ok(None),
            Err(broken) => Err(broken),
        },
    }
}

/// Falls back to a picture's alternative text, which GTK reports as its description.
pub(crate) fn accessible_description(widget: &gtk::Widget) -> String {
    names::naming(widget)
        .and_then(|naming| naming.description)
        .or_else(|| {
            matches!(
                widget.accessible_role(),
                gtk::AccessibleRole::ListItem
                    | gtk::AccessibleRole::GridCell
                    | gtk::AccessibleRole::Cell
            )
            .then(|| state::row_description(widget))
            .flatten()
        })
        .or_else(|| {
            widget
                .downcast_ref::<gtk::Picture>()
                .and_then(gtk::Picture::alternative_text)
                .map(|text| text.to_string())
        })
        .or_else(|| {
            // GtkMenuButton describes its inner toggle button by its own description.
            widget
                .is::<gtk::ToggleButton>()
                .then(|| widget.parent().and_downcast::<gtk::MenuButton>())
                .flatten()
                .map(|menu_button| accessible_description(menu_button.upcast_ref()))
        })
        .unwrap_or_default()
}

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

pub(crate) fn find_presented(root: &gtk::Widget, role: &str, name: &str) -> Option<gtk::Widget> {
    presented(root)
        .into_iter()
        .find(|widget| role_name(widget) == role && accessible_name(widget) == name)
}

pub(crate) fn open_dialog(presented: &[gtk::Widget]) -> Option<gtk::Widget> {
    ["dialog", "alert"].into_iter().find_map(|role| {
        presented
            .iter()
            .find(|widget| role_name(widget) == role && state::rendered_bounds(widget).is_some())
            .cloned()
    })
}

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

pub(crate) fn editable_fields(window: &gtk::Window) -> usize {
    presented(window.upcast_ref())
        .iter()
        .filter(|widget| is_field(widget) && state::rendered_bounds(widget).is_some())
        .count()
}

pub(crate) fn editable_texts(window: &gtk::Window) -> Vec<String> {
    presented(window.upcast_ref())
        .iter()
        .filter(|widget| is_field(widget) && state::rendered_bounds(widget).is_some())
        .filter_map(text_content)
        .collect()
}

pub(crate) fn focused_field(presented: &[gtk::Widget]) -> Option<gtk::Widget> {
    presented
        .iter()
        .find(|widget| {
            is_field(widget) && editable_focused(widget) && state::rendered_bounds(widget).is_some()
        })
        .cloned()
}

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct States {
    pub(crate) selected: bool,
    pub(crate) focused: bool,
    pub(crate) checked: bool,
    pub(crate) pressed: bool,
    pub(crate) sensitive: bool,
    pub(crate) selectable: bool,
    pub(crate) editable: bool,
    pub(crate) focusable: bool,
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
    // GTK's AT-SPI bridge reports an active switch as pressed as well as checked.
    let pressed = if let Some(toggle) = widget.downcast_ref::<gtk::ToggleButton>() {
        toggle.is_active()
    } else if let Some(switch) = widget.downcast_ref::<gtk::Switch>() {
        switch.is_active()
    } else {
        recorded
            .as_ref()
            .and_then(|naming| naming.pressed)
            .unwrap_or(false)
    };
    States {
        // A list box row reports its own selection, as GTK's AT-SPI bridge
        // does, and so does a notebook's tab.
        selected: widget.downcast_ref::<gtk::ListBoxRow>().map_or_else(
            || {
                recorded
                    .as_ref()
                    .and_then(|naming| naming.selected)
                    .or_else(|| notebook_tab_selected(widget))
                    .unwrap_or_else(|| state::row_selected(widget) || list_row_selected(widget))
            },
            gtk::ListBoxRow::is_selected,
        ),
        focused: if widget.is::<gtk::Editable>() {
            editable_focused(widget)
        } else {
            widget.has_focus()
        },
        checked,
        pressed,
        sensitive: widget.is_sensitive(),
        selectable: widget.downcast_ref::<gtk::ListBoxRow>().map_or_else(
            || {
                recorded
                    .as_ref()
                    .is_some_and(|naming| naming.selected.is_some())
            },
            gtk::ListBoxRow::is_selectable,
        ),
        editable: widget
            .dynamic_cast_ref::<gtk::Editable>()
            .map(EditableExt::is_editable)
            .or_else(|| {
                widget
                    .downcast_ref::<gtk::TextView>()
                    .map(gtk::TextView::is_editable)
            })
            .unwrap_or(false),
        focusable: widget.is_focusable(),
    }
}

fn list_row_selected(widget: &gtk::Widget) -> bool {
    matches!(
        widget.accessible_role(),
        gtk::AccessibleRole::ListItem | gtk::AccessibleRole::GridCell | gtk::AccessibleRole::Row
    ) && widget.state_flags().contains(gtk::StateFlags::SELECTED)
}

fn notebook_tab_selected(widget: &gtk::Widget) -> Option<bool> {
    if widget.accessible_role() != gtk::AccessibleRole::Tab {
        return None;
    }
    let label = widget.first_child()?;
    let notebook = std::iter::successors(widget.parent(), WidgetExt::parent)
        .find_map(|ancestor| ancestor.downcast::<gtk::Notebook>().ok())?;
    let page = (0..notebook.n_pages()).find(|page| {
        notebook
            .nth_page(Some(*page))
            .and_then(|child| notebook.tab_label(&child))
            .is_some_and(|tab| tab == label)
    })?;
    Some(notebook.current_page() == Some(page))
}

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

/// Popovers are their own native surfaces, hence the surface offset and screen origin.
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

pub(crate) fn extents(widget: &gtk::Widget, injector: &Injector) -> Result<Rect, LookupError> {
    let broken = |message: &str| LookupError::Broken(message.to_owned());
    let native = widget
        .native()
        .ok_or_else(|| broken("the target has no native surface"))?;
    let bounds = widget
        .compute_bounds(&native)
        .ok_or_else(|| missing("the target has no bounds"))?;
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

/// AT-SPI role names as the Python harness's `tree.Node.role` spells them.
pub(crate) fn role_name(widget: &gtk::Widget) -> &'static str {
    use gtk::AccessibleRole as Role;
    if widget.is::<gtk::ScrolledWindow>() {
        return "scroll pane";
    }
    // GTK's AT-SPI bridge reports a scale as a slider, as `Node.role` reads it.
    if widget.is::<gtk::Scale>() {
        return "slider";
    }
    // And a password entry as password text.
    if widget.is::<gtk::PasswordEntry>() {
        return "password text";
    }
    match widget.accessible_role() {
        Role::AlertDialog | Role::Alert => "alert",
        Role::Button => "button",
        Role::Caption => "caption",
        Role::Cell | Role::GridCell => "table cell",
        Role::Checkbox => "check box",
        Role::ColumnHeader => "column header",
        Role::ComboBox => "combo box",
        Role::Dialog => "dialog",
        Role::Grid | Role::Table => "table",
        Role::Generic | Role::TabPanel => "panel",
        Role::Group => "grouping",
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

/// Tooltips never name a widget here; a control that needs a name gets an explicit label.
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

/// `naming` guards against labelled-by relations that lead back to a widget being named.
fn own_name(widget: &gtk::Widget, naming: &mut Vec<gtk::Widget>) -> Option<String> {
    let recorded = |widget: &gtk::Widget| {
        names::naming(widget)
            .and_then(|naming| naming.label)
            .filter(|label| !label.is_empty())
    };
    // GTK's AT-SPI bridge reports a scrolled window's label on the text view
    // it scrolls, and the scrolled window itself unnamed.
    if widget.is::<gtk::ScrolledWindow>() && recorded(widget).is_some() {
        return Some(String::new());
    }
    // A button's text label names it through the `labelled-by` relation GTK
    // gives it, which comes before a label set on the button itself, unless
    // the relation was replaced or reset.
    if let Some(button) = widget.downcast_ref::<gtk::Button>()
        && button.label().is_some()
        && names::labelled_by(widget).is_empty()
        && !names::labelled_by_reset(widget)
        && let Some(label) = button.child().and_downcast::<gtk::Label>()
    {
        return Some(label.text().to_string());
    }
    if let Some(label) = recorded(widget) {
        return Some(label);
    }
    if widget.is::<gtk::TextView>()
        && let Some(label) = widget
            .parent()
            .filter(|parent| parent.is::<gtk::ScrolledWindow>())
            .and_then(|parent| recorded(&parent))
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
