// SPDX-License-Identifier: MIT

//! One JSON command per line in, one [`Reply`] per line out.
//!
//! A command is an object with exactly one command key (see [`COMMANDS`])
//! whose value is its main argument, plus that command's optional fields. Add
//! a command by adding its fields struct and a [`parse_command`] arm; add a
//! predicate with a [`Check`] variant; add a target kind to [`TARGET_KINDS`]
//! and [`Target`].
//!
//! `tests/qa.rs` compiles this file too, so the runner rejects a malformed
//! step when it loads a scenario instead of the process rejecting it mid-run.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

/// `tree.DEFAULT_TIMEOUT` in `tests/e2e/harness/tree.py`.
pub(crate) const DEFAULT_WAIT: Duration = Duration::from_millis(15_000);
/// `DRAG_STEPS` in `tests/e2e/harness/interaction.py`.
pub(crate) const DRAG_STEPS: u32 = 16;
/// `Pointer.scroll`'s default in `tests/e2e/harness/interaction.py`.
pub(crate) const WHEEL_CLICKS: u32 = 3;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Command {
    Key {
        chord: Chord,
        repeat: u32,
    },
    /// One key per character, as `Keyboard.type_text` types.
    Type(String),
    Click {
        target: Target,
        button: u8,
        count: u32,
        modifiers: Vec<Modifier>,
        at: Option<At>,
    },
    /// Click an entry until it reports itself selected, as `select_entry` does.
    Select(EntryTarget),
    /// Home, then the next-entry key until the entry has focus, as
    /// `select_entry_with_keyboard` does.
    SelectWithKeyboard {
        name: String,
        message: Option<String>,
    },
    /// Clicks the target until `until` holds, clicking again after a second
    /// while it stays closed, as `open_appearance_menu` does.
    OpenMenu {
        target: Target,
        until: Predicate,
        message: Option<String>,
    },
    /// The control's default accessible action, without pointer input, as
    /// `Node.activate` invokes it; the control need not be on screen.
    Activate {
        target: Target,
        message: Option<String>,
    },
    /// Whether a predicate holds now, as `result`; never a failure. The
    /// runner uses it for a step's `when`.
    Holds(Predicate),
    /// Keeps the widget a target names now, as a scenario keeps a node in a
    /// variable; `{"field":"<name>"}` refers to it later.
    Remember {
        target: Target,
        name: String,
    },
    Move {
        target: Target,
        at: Option<At>,
    },
    Wheel {
        target: Target,
        at: Option<At>,
        clicks: u32,
        down: bool,
        modifiers: Vec<Modifier>,
    },
    Drag {
        from: Target,
        to: Target,
        steps: u32,
        release: bool,
        modifiers: Vec<Modifier>,
    },
    /// Lets go of a button a `drag` with `release: false` left held.
    ButtonUp(u8),
    Resize {
        width: u16,
        height: u16,
    },
    /// Waits for a target to stop moving and remembers where it came to rest.
    Settle {
        target: Target,
        timeout: Duration,
    },
    Wait {
        predicate: Predicate,
        timeout: Duration,
        message: Option<String>,
    },
    /// `message` names the twin's assertion, as a wait's names its `strata.wait`.
    Expect {
        predicates: Vec<Predicate>,
        message: Option<String>,
    },
    Query(Query),
}

pub(crate) const COMMANDS: [&str; 18] = [
    "key",
    "type",
    "click",
    "select",
    "select_with_keyboard",
    "open_menu",
    "activate",
    "holds",
    "remember",
    "move",
    "wheel",
    "drag",
    "button_up",
    "resize",
    "settle",
    "wait",
    "expect",
    "query",
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyFields {
    key: String,
    #[serde(default = "one")]
    repeat: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TypeFields {
    #[serde(rename = "type")]
    text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectWithKeyboardFields {
    select_with_keyboard: String,
    message: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenMenuFields {
    open_menu: Target,
    until: Predicate,
    message: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActivateFields {
    activate: Target,
    message: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HoldsFields {
    holds: Predicate,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RememberFields {
    remember: Target,
    #[serde(rename = "as")]
    name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClickFields {
    click: Target,
    #[serde(default = "first_button")]
    button: u8,
    #[serde(default = "one")]
    count: u32,
    #[serde(default)]
    mods: Vec<String>,
    at: Option<At>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectFields {
    select: EntryTarget,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MoveFields {
    #[serde(rename = "move")]
    target: Target,
    at: Option<At>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WheelFields {
    wheel: Target,
    at: Option<At>,
    #[serde(default = "wheel_clicks")]
    clicks: u32,
    #[serde(default)]
    direction: WheelDirection,
    #[serde(default)]
    mods: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum WheelDirection {
    #[default]
    Down,
    Up,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DragFields {
    drag: DragEnds,
    #[serde(default = "drag_steps")]
    steps: u32,
    #[serde(default = "yes")]
    release: bool,
    #[serde(default)]
    mods: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DragEnds {
    from: Target,
    to: Target,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ButtonUpFields {
    button_up: u8,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResizeFields {
    resize: Size,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Size {
    width: u16,
    height: u16,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SettleFields {
    settle: Target,
    timeout_ms: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WaitFields {
    wait: Predicate,
    timeout_ms: Option<u64>,
    message: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectFields {
    expect: Value,
    message: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryFields {
    query: Query,
}

fn one() -> u32 {
    1
}

fn first_button() -> u8 {
    1
}

fn yes() -> bool {
    true
}

fn wheel_clicks() -> u32 {
    WHEEL_CLICKS
}

fn drag_steps() -> u32 {
    DRAG_STEPS
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Query {
    State,
    Tree,
    /// `settings.toml` as JSON, as `read_preferences` reads it.
    Prefs,
}

fn modifiers(names: &[String]) -> Result<Vec<Modifier>, String> {
    names.iter().map(|name| Modifier::parse(name)).collect()
}

pub(crate) fn parse_command(line: &str) -> Result<Command, String> {
    let value: Value =
        serde_json::from_str(line).map_err(|error| format!("invalid JSON: {error}"))?;
    let Value::Object(object) = value else {
        return Err("a command must be a JSON object".to_owned());
    };
    let names: Vec<&str> = COMMANDS
        .into_iter()
        .filter(|name| object.contains_key(*name))
        .collect();
    let [name] = names.as_slice() else {
        return Err(format!(
            "a command needs exactly one of {COMMANDS:?}, found {names:?}"
        ));
    };
    let fields = Value::Object(object.clone());
    let invalid = |error: serde_json::Error| format!("invalid {name} command: {error}");
    match *name {
        "key" => {
            let fields: KeyFields = serde_json::from_value(fields).map_err(invalid)?;
            Ok(Command::Key {
                chord: Chord::parse(&fields.key)?,
                repeat: fields.repeat,
            })
        }
        "type" => {
            let fields: TypeFields = serde_json::from_value(fields).map_err(invalid)?;
            if fields.text.is_empty() {
                return Err("type needs text".to_owned());
            }
            Ok(Command::Type(fields.text))
        }
        "select_with_keyboard" => {
            let fields: SelectWithKeyboardFields =
                serde_json::from_value(fields).map_err(invalid)?;
            Ok(Command::SelectWithKeyboard {
                name: fields.select_with_keyboard,
                message: fields.message,
            })
        }
        "open_menu" => {
            let fields: OpenMenuFields = serde_json::from_value(fields).map_err(invalid)?;
            fields.open_menu.accepts(None)?;
            Ok(Command::OpenMenu {
                target: fields.open_menu,
                until: fields.until,
                message: fields.message,
            })
        }
        "activate" => {
            let fields: ActivateFields = serde_json::from_value(fields).map_err(invalid)?;
            if !matches!(fields.activate, Target::Role { .. }) {
                return Err("activate takes a role target".to_owned());
            }
            Ok(Command::Activate {
                target: fields.activate,
                message: fields.message,
            })
        }
        "holds" => {
            let fields: HoldsFields = serde_json::from_value(fields).map_err(invalid)?;
            Ok(Command::Holds(fields.holds))
        }
        "remember" => {
            let fields: RememberFields = serde_json::from_value(fields).map_err(invalid)?;
            if !matches!(fields.remember, Target::Field(None)) {
                return Err("remember keeps the focused field: {\"field\":true}".to_owned());
            }
            if fields.name.is_empty() {
                return Err("remember needs a name in as".to_owned());
            }
            Ok(Command::Remember {
                target: fields.remember,
                name: fields.name,
            })
        }
        "click" => {
            let fields: ClickFields = serde_json::from_value(fields).map_err(invalid)?;
            if !matches!(fields.button, 1 | 3) || !matches!(fields.count, 1 | 2) {
                return Err("click takes button 1 or 3 and count 1 or 2".to_owned());
            }
            fields.click.accepts(fields.at.as_ref())?;
            Ok(Command::Click {
                target: fields.click,
                button: fields.button,
                count: fields.count,
                modifiers: modifiers(&fields.mods)?,
                at: fields.at,
            })
        }
        "select" => {
            let fields: SelectFields = serde_json::from_value(fields).map_err(invalid)?;
            Ok(Command::Select(fields.select))
        }
        "move" => {
            let fields: MoveFields = serde_json::from_value(fields).map_err(invalid)?;
            fields.target.accepts(fields.at.as_ref())?;
            Ok(Command::Move {
                target: fields.target,
                at: fields.at,
            })
        }
        "wheel" => {
            let fields: WheelFields = serde_json::from_value(fields).map_err(invalid)?;
            if fields.clicks == 0 {
                return Err("wheel needs at least one click".to_owned());
            }
            fields.wheel.accepts(fields.at.as_ref())?;
            Ok(Command::Wheel {
                target: fields.wheel,
                at: fields.at,
                clicks: fields.clicks,
                down: matches!(fields.direction, WheelDirection::Down),
                modifiers: modifiers(&fields.mods)?,
            })
        }
        "drag" => {
            let fields: DragFields = serde_json::from_value(fields).map_err(invalid)?;
            if fields.steps == 0 {
                return Err("drag needs at least one step".to_owned());
            }
            Ok(Command::Drag {
                from: fields.drag.from,
                to: fields.drag.to,
                steps: fields.steps,
                release: fields.release,
                modifiers: modifiers(&fields.mods)?,
            })
        }
        "button_up" => {
            let fields: ButtonUpFields = serde_json::from_value(fields).map_err(invalid)?;
            if !matches!(fields.button_up, 1..=3) {
                return Err("button_up takes button 1, 2, or 3".to_owned());
            }
            Ok(Command::ButtonUp(fields.button_up))
        }
        "resize" => {
            let fields: ResizeFields = serde_json::from_value(fields).map_err(invalid)?;
            if fields.resize.width == 0 || fields.resize.height == 0 {
                return Err("resize needs a positive width and height".to_owned());
            }
            Ok(Command::Resize {
                width: fields.resize.width,
                height: fields.resize.height,
            })
        }
        "settle" => {
            let fields: SettleFields = serde_json::from_value(fields).map_err(invalid)?;
            Ok(Command::Settle {
                target: fields.settle,
                timeout: fields
                    .timeout_ms
                    .map_or(DEFAULT_WAIT, Duration::from_millis),
            })
        }
        "wait" => {
            let fields: WaitFields = serde_json::from_value(fields).map_err(invalid)?;
            Ok(Command::Wait {
                predicate: fields.wait,
                timeout: fields
                    .timeout_ms
                    .map_or(DEFAULT_WAIT, Duration::from_millis),
                message: fields.message,
            })
        }
        "expect" => {
            let fields: ExpectFields = serde_json::from_value(fields).map_err(invalid)?;
            let predicates = match fields.expect {
                Value::Array(items) if items.is_empty() => {
                    return Err("expect needs a predicate".to_owned());
                }
                Value::Array(items) => items
                    .into_iter()
                    .map(serde_json::from_value)
                    .collect::<Result<Vec<Predicate>, _>>()
                    .map_err(invalid)?,
                one => vec![serde_json::from_value(one).map_err(invalid)?],
            };
            Ok(Command::Expect {
                predicates,
                message: fields.message,
            })
        }
        _ => {
            let fields: QueryFields = serde_json::from_value(fields).map_err(invalid)?;
            Ok(Command::Query(fields.query))
        }
    }
}

/// Where on a target a pointer event lands: `[x, y]` from the target's
/// top-left corner, each a pixel offset (negative counts back from the far
/// edge) or a share of the extent such as `"80%"`. Without it, the centre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct At {
    pub(crate) x: Offset,
    pub(crate) y: Offset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Offset {
    Pixels(i32),
    Percent(u8),
}

impl Offset {
    fn parse(value: &Value) -> Result<Self, String> {
        if let Some(pixels) = value.as_i64() {
            return i32::try_from(pixels)
                .map(Self::Pixels)
                .map_err(|_| format!("{pixels} is out of range"));
        }
        let percent = value
            .as_str()
            .and_then(|text| text.strip_suffix('%'))
            .and_then(|number| number.parse::<u8>().ok())
            .filter(|percent| *percent <= 100);
        percent.map(Self::Percent).ok_or_else(|| {
            format!("an offset is whole pixels or a percentage like \"80%\", not {value}")
        })
    }

    /// From a box starting at `start` and `extent` long.
    pub(crate) fn resolve(self, start: f64, extent: f64) -> f64 {
        match self {
            Self::Pixels(pixels) if pixels < 0 => start + extent + f64::from(pixels),
            Self::Pixels(pixels) => start + f64::from(pixels),
            Self::Percent(percent) => start + (extent * f64::from(percent) / 100.0).floor(),
        }
    }
}

impl<'de> Deserialize<'de> for At {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        let parsed = match value.as_array().map(Vec::as_slice) {
            Some([x, y]) => Offset::parse(x).and_then(|x| {
                Ok(Self {
                    x,
                    y: Offset::parse(y)?,
                })
            }),
            _ => Err(format!("at is [x, y], not {value}")),
        };
        parsed.map_err(serde::de::Error::custom)
    }
}

/// The target kinds, each named by the one key that selects it.
pub(crate) const TARGET_KINDS: [&str; 11] = [
    "entry",
    "result",
    "role",
    "background",
    "pane",
    "viewport",
    "field",
    "sidebar",
    "header",
    "menu_item",
    "dialog_button",
];

/// Names a widget the way a person sees it, never by screen position. Each
/// mirrors a page-object lookup in `tests/e2e/harness/browser.py`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    /// An entry row; `entry(name, directory)`.
    Entry(EntryTarget),
    /// The first showing widget with this AT-SPI role name and accessible name.
    Role { role: String, name: String },
    /// The entry list below its last entry; `background_point(directory)`.
    Background { dir: Option<String> },
    /// A browser pane; points at `empty_point(directory)` unless `at` is given.
    Pane { dir: Option<String> },
    /// The scroll viewport around a pane's entry list.
    Viewport { dir: Option<String> },
    /// The focused editable text, `editable_field()`; or with a name, the
    /// field a `remember` kept under it.
    Field(Option<String>),
    /// A filter result by its path below the filtered directory, as
    /// `test_filter_results.result` finds it.
    Result(String),
    /// `sidebar_button(name)`: a button by name.
    Sidebar(String),
    /// `header_button(name)`: a button by name.
    Header(String),
    /// `menu_item(label)`: a menu item, in the open menu if there is one.
    MenuItem(String),
    /// `dialog_button(label)`: a button in the open dialog.
    DialogButton(String),
}

impl Target {
    fn accepts(&self, at: Option<&At>) -> Result<(), String> {
        if at.is_some() && matches!(self, Self::Background { .. }) {
            return Err("at does not apply to a background target, which is a point".to_owned());
        }
        Ok(())
    }

    fn from_object(object: Map<String, Value>) -> Result<Self, String> {
        let kinds: Vec<&str> = TARGET_KINDS
            .into_iter()
            .filter(|kind| object.contains_key(*kind))
            .collect();
        let [kind] = kinds.as_slice() else {
            return Err(format!(
                "a target needs exactly one of {TARGET_KINDS:?}, found {kinds:?}"
            ));
        };
        let value = Value::Object(object);
        let invalid = |error: serde_json::Error| format!("invalid {kind} target: {error}");
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RoleFields {
            role: String,
            name: String,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct BackgroundFields {
            background: bool,
            dir: Option<String>,
        }
        let directory = |value: Value| -> Result<Option<String>, String> {
            let mut object = value.as_object().cloned().unwrap_or_default();
            if object.len() != 1 {
                return Err(format!("invalid {kind} target: it takes only {kind}"));
            }
            serde_json::from_value(object.remove(*kind).unwrap_or(Value::Null)).map_err(invalid)
        };
        let label = |value: Value| -> Result<String, String> {
            directory(value)?.ok_or_else(|| format!("invalid {kind} target: {kind} needs a label"))
        };
        match *kind {
            "entry" => serde_json::from_value(value)
                .map(Self::Entry)
                .map_err(invalid),
            "role" => {
                let fields: RoleFields = serde_json::from_value(value).map_err(invalid)?;
                Ok(Self::Role {
                    role: fields.role,
                    name: fields.name,
                })
            }
            "background" => {
                let fields: BackgroundFields = serde_json::from_value(value).map_err(invalid)?;
                if !fields.background {
                    return Err("invalid background target: background must be true".to_owned());
                }
                Ok(Self::Background { dir: fields.dir })
            }
            "field" => match value
                .as_object()
                .and_then(|object| (object.len() == 1).then(|| object.get("field")).flatten())
            {
                Some(Value::Bool(true)) => Ok(Self::Field(None)),
                Some(Value::String(name)) if !name.is_empty() => {
                    Ok(Self::Field(Some(name.clone())))
                }
                _ => Err(
                    "invalid field target: field is true or the name a remember gave it".to_owned(),
                ),
            },
            "result" => label(value).map(Self::Result),
            "pane" => directory(value).map(|dir| Self::Pane { dir }),
            "viewport" => directory(value).map(|dir| Self::Viewport { dir }),
            "sidebar" => label(value).map(Self::Sidebar),
            "header" => label(value).map(Self::Header),
            "menu_item" => label(value).map(Self::MenuItem),
            _ => label(value).map(Self::DialogButton),
        }
    }

    pub(crate) fn to_json(&self) -> Value {
        match self {
            Self::Entry(entry) => json!(entry),
            Self::Role { role, name } => json!({ "role": role, "name": name }),
            Self::Background { dir: None } => json!({ "background": true }),
            Self::Background { dir } => json!({ "background": true, "dir": dir }),
            Self::Pane { dir } => json!({ "pane": dir }),
            Self::Viewport { dir } => json!({ "viewport": dir }),
            Self::Field(None) => json!({ "field": true }),
            Self::Field(Some(name)) => json!({ "field": name }),
            Self::Result(path) => json!({ "result": path }),
            Self::Sidebar(name) => json!({ "sidebar": name }),
            Self::Header(name) => json!({ "header": name }),
            Self::MenuItem(name) => json!({ "menu_item": name }),
            Self::DialogButton(name) => json!({ "dialog_button": name }),
        }
    }
}

impl<'de> Deserialize<'de> for Target {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match Value::deserialize(deserializer)? {
            Value::Object(object) => Self::from_object(object),
            other => Err(format!(
                "a target is an object such as {{\"entry\":\"todo.txt\"}}, not {other}"
            )),
        }
        .map_err(serde::de::Error::custom)
    }
}

impl Serialize for Target {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_json().serialize(serializer)
    }
}

/// An entry row by name, in the named pane or in any pane on screen.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EntryTarget {
    pub(crate) entry: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) dir: Option<String>,
}

/// An expectation about window state, shared by `wait` and `expect`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Predicate {
    #[serde(flatten)]
    pub(crate) check: Check,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) dir: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Check {
    Directory(Option<String>),
    Panes(Vec<String>),
    /// The deepest pane's name; `strata.pane().name`.
    Pane(Option<String>),
    Entries(Names),
    /// An entry is listed, in `dir` or in any pane; `strata.entry(name)`.
    Entry(String),
    /// `wait_for_entry_gone(name, dir)`.
    EntryGone(String),
    Selected(Names),
    /// The focused entry, `focused_name()`: a name, `true` for any, or null.
    Focused(Presence),
    /// The pane holding keyboard focus, `focused_pane()`.
    FocusedPane(Presence),
    ViewMode(String),
    WindowWidth(i32),
    /// The open dialog, `dialog()`: its name, `true` for any, or null.
    Dialog(Presence),
    /// The open menu's items, `menu_items()`; null while no menu is open.
    MenuItems(Option<Names>),
    /// Text the quick preview shows, `preview_shows(text)`; null while closed.
    Preview(Option<String>),
    /// What a recursive filter lists, `matches(dir)`.
    Matches(Names),
    /// A field's text, caret, selection, and focus.
    Field(FieldCheck),
    /// A key in `settings.toml`, as `read_preferences` reads it; null when absent.
    Pref(PrefCheck),
    /// The field lies inside the deepest pane and the window, as the caret
    /// scenario's `assert_editor_constrained` checks.
    EditorWithinViewport(FieldRef),
    /// A target's accessible states, `node.has_state(...)`.
    State(StateCheck),
    /// Holds when any of the predicates holds.
    Any(Vec<Predicate>),
    /// A target can be found on screen; `window.find(...) is not None`.
    Present(Target),
    /// `window.find(...) is None`.
    Absent(Target),
    /// Where a target is now, against where the last `settle` on it ended.
    Moved(Movement),
}

impl Check {
    fn takes_directory(&self) -> bool {
        matches!(
            self,
            Self::Entries(_)
                | Self::Selected(_)
                | Self::Entry(_)
                | Self::EntryGone(_)
                | Self::Matches(_)
        )
    }

    /// Checks that resolve targets against live widgets or files rather than a snapshot.
    pub(crate) fn is_live(&self) -> bool {
        match self {
            Self::Present(_)
            | Self::Absent(_)
            | Self::Moved(_)
            | Self::Field(_)
            | Self::Pref(_)
            | Self::EditorWithinViewport(_)
            | Self::State(_) => true,
            Self::Any(predicates) => predicates.iter().any(|predicate| predicate.check.is_live()),
            _ => false,
        }
    }
}

/// Null, `true` for anything, or a name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Presence {
    Absent,
    Any,
    Named(String),
}

impl Presence {
    pub(crate) fn matches(&self, actual: Option<&str>) -> bool {
        match self {
            Self::Absent => actual.is_none(),
            Self::Any => actual.is_some(),
            Self::Named(name) => actual == Some(name.as_str()),
        }
    }
}

impl<'de> Deserialize<'de> for Presence {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match Value::deserialize(deserializer)? {
            Value::Null => Ok(Self::Absent),
            Value::Bool(true) => Ok(Self::Any),
            Value::String(name) => Ok(Self::Named(name)),
            other => Err(serde::de::Error::custom(format!(
                "expected a name, true for any, or null, not {other}"
            ))),
        }
    }
}

impl Serialize for Presence {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Absent => serializer.serialize_none(),
            Self::Any => serializer.serialize_bool(true),
            Self::Named(name) => serializer.serialize_str(name),
        }
    }
}

/// Which field a check reads: one a `remember` kept, the first on screen
/// with this accessible name, or else the focused one.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FieldRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) of: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
}

/// `{}` holds whenever the field exists; each given property must match.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FieldCheck {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) of: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) text: Option<TextMatch>,
    /// The caret's character offset, `Text.get_caret_offset`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) caret: Option<CaretMatch>,
    /// `[start, end]` in characters, or null for no selection.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) selection: Option<Option<[i32; 2]>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) focused: Option<bool>,
}

impl FieldCheck {
    pub(crate) fn field(&self) -> FieldRef {
        FieldRef {
            of: self.of.clone(),
            name: self.name.clone(),
        }
    }
}

/// Tells an explicit null apart from a missing key.
fn present<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}

/// Exact text, or text with this start, end, or part.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub(crate) enum TextMatch {
    Exactly(String),
    Like(TextPattern),
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TextPattern {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) starts_with: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) ends_with: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) contains: Option<String>,
}

impl TextMatch {
    pub(crate) fn matches(&self, actual: &str) -> bool {
        match self {
            Self::Exactly(text) => text == actual,
            Self::Like(pattern) => {
                pattern
                    .starts_with
                    .as_deref()
                    .is_none_or(|start| actual.starts_with(start))
                    && pattern
                        .ends_with
                        .as_deref()
                        .is_none_or(|end| actual.ends_with(end))
                    && pattern
                        .contains
                        .as_deref()
                        .is_none_or(|part| actual.contains(part))
            }
        }
    }
}

/// An exact offset, or an inclusive range.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub(crate) enum CaretMatch {
    Exactly(i32),
    Between { min: i32, max: i32 },
}

impl CaretMatch {
    pub(crate) fn matches(&self, actual: i32) -> bool {
        match self {
            Self::Exactly(offset) => *offset == actual,
            Self::Between { min, max } => (*min..=*max).contains(&actual),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PrefCheck {
    pub(crate) key: String,
    pub(crate) value: Value,
}

/// Each given state must match; the names are AT-SPI's.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StateCheck {
    pub(crate) target: Target,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) selected: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) focused: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) checked: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) pressed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) sensitive: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Movement {
    pub(crate) target: Target,
    pub(crate) direction: Direction,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Direction {
    Up,
    Down,
    Left,
    Right,
    /// The bounds are exactly where they settled.
    None,
}

/// A name list in visual order, or names it must or must not contain.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub(crate) enum Names {
    Exactly(Vec<String>),
    Including(NameFilter),
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NameFilter {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) contains: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) excludes: Vec<String>,
    /// How many names, duplicates included.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) count: Option<usize>,
}

impl Names {
    pub(crate) fn matches(&self, actual: &[String]) -> bool {
        match self {
            Self::Exactly(expected) => expected == actual,
            Self::Including(filter) => {
                filter.contains.iter().all(|name| actual.contains(name))
                    && !filter.excludes.iter().any(|name| actual.contains(name))
                    && filter.count.is_none_or(|count| count == actual.len())
            }
        }
    }
}

impl<'de> Deserialize<'de> for Predicate {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut object = Map::<String, Value>::deserialize(deserializer)?;
        let dir = match object.remove("dir") {
            None | Some(Value::Null) => None,
            Some(Value::String(dir)) => Some(dir),
            Some(other) => {
                return Err(serde::de::Error::custom(format!(
                    "dir must be a string, not {other}"
                )));
            }
        };
        if object.len() != 1 {
            let keys: Vec<&String> = object.keys().collect();
            return Err(serde::de::Error::custom(format!(
                "a predicate needs exactly one check, found {keys:?}"
            )));
        }
        let check: Check =
            serde_json::from_value(Value::Object(object)).map_err(serde::de::Error::custom)?;
        let field = match &check {
            Check::Field(check) => Some(check.field()),
            Check::EditorWithinViewport(field) => Some(field.clone()),
            _ => None,
        };
        if field.is_some_and(|field| field.of.is_some() && field.name.is_some()) {
            return Err(serde::de::Error::custom(
                "a field is either remembered (of) or found by name, not both",
            ));
        }
        if dir.is_some() && !check.takes_directory() {
            return Err(serde::de::Error::custom(
                "dir applies only to entries, entry, entry_gone, selected, and matches",
            ));
        }
        Ok(Self { check, dir })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Modifier {
    Ctrl,
    Shift,
    Alt,
    AltGr,
}

impl Modifier {
    fn parse(name: &str) -> Result<Self, String> {
        match name.to_ascii_lowercase().as_str() {
            "ctrl" => Ok(Self::Ctrl),
            "shift" => Ok(Self::Shift),
            "alt" => Ok(Self::Alt),
            "altgr" => Ok(Self::AltGr),
            _ => Err(format!("unknown modifier {name:?}")),
        }
    }

    /// The US keys the Python harness holds; `altgr` is the right Alt key,
    /// which AltGr layouts turn into AltGr.
    pub(crate) fn keysym_name(self) -> &'static str {
        match self {
            Self::Ctrl => "Control_L",
            Self::Shift => "Shift_L",
            Self::Alt => "Alt_L",
            Self::AltGr => "Alt_R",
        }
    }
}

/// A key in the harness spelling: `Return`, `j`, `ctrl+shift+n`; a trailing
/// `+` is the plus key itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Chord {
    pub(crate) modifiers: Vec<Modifier>,
    pub(crate) key: String,
}

impl Chord {
    pub(crate) fn parse(spelling: &str) -> Result<Self, String> {
        if spelling.is_empty() {
            return Err("an empty key".to_owned());
        }
        let parts: Vec<&str> = spelling.split('+').collect();
        let (key, modifiers) = match parts.split_last() {
            Some((&"", modifiers)) => ("+", modifiers),
            Some((key, modifiers)) => (*key, modifiers),
            None => return Err("an empty key".to_owned()),
        };
        let modifiers = modifiers
            .iter()
            .filter(|part| !part.is_empty())
            .map(|part| Modifier::parse(part))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            modifiers,
            key: key.to_owned(),
        })
    }
}

/// Mismatches carry the observed value so a failing step explains itself.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct Failure {
    pub(crate) predicate: Value,
    pub(crate) actual: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) message: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct Reply {
    pub(crate) ok: bool,
    pub(crate) app: Value,
    pub(crate) changed: Vec<String>,
    pub(crate) criticals: Vec<String>,
    pub(crate) result: Value,
    pub(crate) failures: Vec<Failure>,
    pub(crate) error: Option<String>,
}

/// The top-level `app` keys whose values differ from the previous snapshot.
pub(crate) fn changed_keys(previous: Option<&Value>, current: &Value) -> Vec<String> {
    let Value::Object(current) = current else {
        return Vec::new();
    };
    let previous = previous.and_then(Value::as_object);
    current
        .iter()
        .filter(|(key, value)| previous.and_then(|previous| previous.get(*key)) != Some(*value))
        .map(|(key, _)| key.clone())
        .collect()
}
