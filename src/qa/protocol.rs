// SPDX-License-Identifier: MIT

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
    Type(String),
    Click {
        target: Target,
        button: u8,
        count: u32,
        modifiers: Vec<Modifier>,
        at: Option<At>,
        left_bound: Option<String>,
        release_modifiers_first: bool,
    },
    ClickResultRows {
        states: RowStates,
        modifiers: Vec<Modifier>,
    },
    Select(EntryTarget),
    SelectWithKeyboard {
        name: String,
        message: Option<String>,
    },
    OpenMenu {
        target: Target,
        until: Predicate,
        message: Option<String>,
        open_states: Vec<StateName>,
    },
    Adopt {
        title: String,
        timeout: Duration,
        message: Option<String>,
    },
    Keep {
        rect: [i64; 4],
        name: String,
    },
    RevealControl {
        target: Target,
    },
    Reveal {
        target: Target,
    },
    Activate {
        target: Target,
        message: Option<String>,
    },
    Holds(Predicate),
    Remember {
        target: Target,
        name: String,
    },
    Note {
        value: Accessor,
        dir: Option<String>,
        wait: Option<Duration>,
        message: Option<String>,
        from: Option<String>,
        through: Option<String>,
        unless_first: Option<String>,
        of: Option<Target>,
        middle: Option<usize>,
        plus: Vec<String>,
        sorted: bool,
        pick: Option<i64>,
        expr: Option<Expr>,
        until: Vec<Bound>,
        until_names: Option<Names>,
    },
    Move {
        target: Target,
        at: Option<At>,
        left_bound: Option<String>,
    },
    MoveBy {
        x: i16,
        y: i16,
    },
    Wheel {
        target: Target,
        at: Option<At>,
        clicks: u32,
        down: bool,
        modifiers: Vec<Modifier>,
        until: Option<Repeat>,
    },
    KeysUntil {
        predicate: Predicate,
        keys: Vec<Chord>,
        times: u32,
        check_first: bool,
        await_focus_change: Option<(Duration, Option<String>)>,
        exhausted: Option<String>,
    },
    KeyDown(String),
    KeyUp(String),
    During {
        checks: Vec<(Predicate, Option<String>)>,
        duration: Duration,
        every: Duration,
    },
    Capture {
        target: Target,
        inset: u16,
        name: String,
    },
    MeasureText {
        kept: String,
        name: String,
    },
    DropSamples {
        kept: String,
        resting: String,
        dragging: String,
        escape: bool,
        name: String,
    },
    Drag {
        from: Target,
        to: Target,
        from_at: Option<At>,
        to_at: Option<At>,
        steps: u32,
        release: bool,
        modifiers: Vec<Modifier>,
        after_press: Vec<PressCheck>,
    },
    ButtonDown(u8),
    ButtonUp(u8),
    SetScrollbar {
        orientation: Orientation,
        value: f64,
    },
    Sleep(Duration),
    Resize {
        width: u16,
        height: Option<u16>,
    },
    Settle {
        target: Target,
        timeout: Duration,
        name: Option<String>,
    },
    Wait {
        predicate: Predicate,
        timeout: Duration,
        message: Option<String>,
        samples: usize,
        settle: Option<Target>,
        never: Option<Box<(Predicate, Option<String>)>>,
    },
    Expect {
        predicates: Vec<Predicate>,
        message: Option<String>,
        timeout: Option<Duration>,
        hold: Option<Duration>,
    },
    ClipboardSet(String),
    Clipboard {
        text: TextMatch,
        message: Option<String>,
    },
    Query(Query),
}

pub(crate) const COMMANDS: [&str; 37] = [
    "key",
    "adopt",
    "keep",
    "reveal_control",
    "reveal",
    "clipboard_set",
    "clipboard",
    "click_result_rows",
    "keys_until",
    "key_down",
    "key_up",
    "during",
    "capture",
    "measure_text",
    "drop_samples",
    "type",
    "click",
    "select",
    "select_with_keyboard",
    "open_menu",
    "activate",
    "holds",
    "remember",
    "note",
    "move",
    "move_by",
    "wheel",
    "drag",
    "button_down",
    "button_up",
    "set_scrollbar",
    "sleep",
    "resize",
    "settle",
    "wait",
    "expect",
    "query",
];

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Repeat {
    pub(crate) predicate: Predicate,
    pub(crate) tries: Option<u32>,
    pub(crate) timeout: Duration,
    pub(crate) message: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PressCheck {
    pub(crate) predicate: Predicate,
    pub(crate) wait: Option<Duration>,
    pub(crate) message: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PressStepFields {
    wait: Option<Predicate>,
    expect: Option<Predicate>,
    timeout_ms: Option<u64>,
    message: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeysUntilFields {
    keys_until: Predicate,
    keys: Vec<String>,
    times: Value,
    #[serde(default)]
    check_first: bool,
    await_focus_change: Option<AwaitChange>,
    exhausted: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AwaitChange {
    timeout_ms: u64,
    message: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdoptFields {
    adopt: String,
    timeout_ms: Option<u64>,
    message: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeepFields {
    keep: [i64; 4],
    #[serde(rename = "as")]
    name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RevealControlFields {
    reveal_control: Target,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RevealFields {
    reveal: Target,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyDownFields {
    key_down: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyUpFields {
    key_up: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DuringFields {
    during: Value,
    ms: u64,
    #[serde(default = "poll_ms")]
    every_ms: u64,
    message: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CaptureFields {
    capture: Target,
    #[serde(default)]
    inset: u16,
    #[serde(rename = "as")]
    name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MeasureTextFields {
    measure_text: String,
    #[serde(rename = "as")]
    name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DropSamplesFields {
    drop_samples: String,
    resting: String,
    dragging: String,
    #[serde(default)]
    escape: bool,
    #[serde(rename = "as")]
    name: String,
}

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
    open_states: Option<Vec<StateName>>,
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
struct NoteFields {
    note: Accessor,
    /// The runner keeps the result under this name; the process ignores it.
    #[serde(rename = "as")]
    name: String,
    dir: Option<String>,
    #[serde(default)]
    until_found: bool,
    timeout_ms: Option<u64>,
    message: Option<String>,
    from: Option<String>,
    through: Option<String>,
    unless_first: Option<String>,
    of: Option<Target>,
    middle: Option<usize>,
    #[serde(default)]
    plus: Vec<String>,
    #[serde(default)]
    sorted: bool,
    pick: Option<i64>,
    expr: Option<Expr>,
    #[serde(default)]
    until: Vec<Bound>,
    until_names: Option<Names>,
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
    left_bound: Option<String>,
    #[serde(default)]
    release_mods_first: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClickResultRowsFields {
    click_result_rows: RowStates,
    #[serde(default)]
    mods: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RowStates {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) selected: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) focused: Option<bool>,
}

impl RowStates {
    pub(crate) fn matches(&self, selected: bool, focused: bool) -> bool {
        self.selected.is_none_or(|wanted| wanted == selected)
            && self.focused.is_none_or(|wanted| wanted == focused)
    }
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
    left_bound: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MoveByFields {
    move_by: [i16; 2],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ButtonDownFields {
    button_down: u8,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetScrollbarFields {
    set_scrollbar: Orientation,
    value: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SleepFields {
    sleep: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Orientation {
    Horizontal,
    Vertical,
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
    until: Option<Predicate>,
    tries: Option<u32>,
    timeout_ms: Option<u64>,
    message: Option<String>,
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
    after_press: Option<Value>,
    message: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DragEnds {
    from: Target,
    to: Target,
    from_at: Option<At>,
    to_at: Option<At>,
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
    height: Option<u16>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SettleFields {
    settle: Target,
    timeout_ms: Option<u64>,
    #[serde(rename = "as")]
    name: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WaitFields {
    wait: Predicate,
    timeout_ms: Option<u64>,
    message: Option<String>,
    #[serde(default = "one_sample")]
    samples: usize,
    settling: Option<Target>,
    never: Option<Predicate>,
    never_message: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DuringCheck {
    check: Predicate,
    message: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectFields {
    expect: Value,
    message: Option<String>,
    timeout_ms: Option<u64>,
    hold_ms: Option<u64>,
    /// The runner keeps this flag for a later `if` or `unless`; the process ignores it.
    #[serde(rename = "as")]
    _name: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClipboardSetFields {
    clipboard_set: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClipboardFields {
    clipboard: TextMatch,
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

fn one_sample() -> usize {
    1
}

/// `tree.POLL_INTERVAL`, in milliseconds.
fn poll_ms() -> u64 {
    50
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
        "key_down" | "key_up" => {
            let key = if *name == "key_down" {
                serde_json::from_value::<KeyDownFields>(fields)
                    .map_err(invalid)?
                    .key_down
            } else {
                serde_json::from_value::<KeyUpFields>(fields)
                    .map_err(invalid)?
                    .key_up
            };
            if key.is_empty() || key.contains('+') && key != "+" {
                return Err(format!("{name} takes one key name, not {key:?}"));
            }
            Ok(if *name == "key_down" {
                Command::KeyDown(key)
            } else {
                Command::KeyUp(key)
            })
        }
        "clipboard_set" => {
            let fields: ClipboardSetFields = serde_json::from_value(fields).map_err(invalid)?;
            Ok(Command::ClipboardSet(fields.clipboard_set))
        }
        "clipboard" => {
            let fields: ClipboardFields = serde_json::from_value(fields).map_err(invalid)?;
            Ok(Command::Clipboard {
                text: fields.clipboard,
                message: fields.message,
            })
        }
        "keys_until" => {
            let fields: KeysUntilFields = serde_json::from_value(fields).map_err(invalid)?;
            let times = match &fields.times {
                Value::Array(items) => u32::try_from(items.len()).ok(),
                other => other.as_u64().and_then(|times| u32::try_from(times).ok()),
            }
            .ok_or("keys_until times is a number of rounds or a noted list")?;
            if fields.keys.is_empty() || fields.times == 0 {
                return Err("keys_until needs keys and at least one round".to_owned());
            }
            Ok(Command::KeysUntil {
                predicate: fields.keys_until,
                keys: fields
                    .keys
                    .iter()
                    .map(|key| Chord::parse(key))
                    .collect::<Result<_, _>>()?,
                times,
                check_first: fields.check_first,
                await_focus_change: fields
                    .await_focus_change
                    .map(|change| (Duration::from_millis(change.timeout_ms), change.message)),
                exhausted: fields.exhausted,
            })
        }
        "adopt" => {
            let fields: AdoptFields = serde_json::from_value(fields).map_err(invalid)?;
            if fields.adopt.is_empty() {
                return Err("adopt needs a window title".to_owned());
            }
            Ok(Command::Adopt {
                title: fields.adopt,
                timeout: fields
                    .timeout_ms
                    .map_or(DEFAULT_WAIT, Duration::from_millis),
                message: fields.message,
            })
        }
        "keep" => {
            let fields: KeepFields = serde_json::from_value(fields).map_err(invalid)?;
            if fields.name.is_empty() {
                return Err("keep needs a name in as".to_owned());
            }
            Ok(Command::Keep {
                rect: fields.keep,
                name: fields.name,
            })
        }
        "reveal_control" | "reveal" => {
            let target = if *name == "reveal" {
                serde_json::from_value::<RevealFields>(fields)
                    .map_err(invalid)?
                    .reveal
            } else {
                serde_json::from_value::<RevealControlFields>(fields)
                    .map_err(invalid)?
                    .reveal_control
            };
            let Target::Role {
                name: Some(_),
                within: None,
                ..
            } = &target
            else {
                return Err(format!("{name} takes a named role target without within"));
            };
            Ok(if *name == "reveal" {
                Command::Reveal { target }
            } else {
                Command::RevealControl { target }
            })
        }
        "during" => {
            let fields: DuringFields = serde_json::from_value(fields).map_err(invalid)?;
            if fields.ms == 0 || fields.every_ms == 0 {
                return Err("during needs a positive ms and every_ms".to_owned());
            }
            let checks = match fields.during {
                Value::Array(items) => {
                    if items.is_empty() || fields.message.is_some() {
                        return Err(
                            "a during list needs checks, each with its own message".to_owned()
                        );
                    }
                    items
                        .into_iter()
                        .map(|item| {
                            serde_json::from_value::<DuringCheck>(item)
                                .map(|check| (check.check, check.message))
                                .map_err(invalid)
                        })
                        .collect::<Result<_, _>>()?
                }
                one => vec![(
                    serde_json::from_value(one).map_err(invalid)?,
                    fields.message,
                )],
            };
            Ok(Command::During {
                checks,
                duration: Duration::from_millis(fields.ms),
                every: Duration::from_millis(fields.every_ms),
            })
        }
        "capture" => {
            let fields: CaptureFields = serde_json::from_value(fields).map_err(invalid)?;
            if fields.name.is_empty() {
                return Err("capture needs a name in as".to_owned());
            }
            Ok(Command::Capture {
                target: fields.capture,
                inset: fields.inset,
                name: fields.name,
            })
        }
        "measure_text" => {
            let fields: MeasureTextFields = serde_json::from_value(fields).map_err(invalid)?;
            if fields.name.is_empty() {
                return Err("measure_text needs a name in as".to_owned());
            }
            Ok(Command::MeasureText {
                kept: fields.measure_text,
                name: fields.name,
            })
        }
        "drop_samples" => {
            let fields: DropSamplesFields = serde_json::from_value(fields).map_err(invalid)?;
            if fields.name.is_empty() {
                return Err("drop_samples needs a name in as".to_owned());
            }
            Ok(Command::DropSamples {
                kept: fields.drop_samples,
                resting: fields.resting,
                dragging: fields.dragging,
                escape: fields.escape,
                name: fields.name,
            })
        }
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
            let open_states = fields
                .open_states
                .unwrap_or_else(|| vec![StateName::Checked]);
            if open_states.is_empty() {
                return Err("open_states needs a state".to_owned());
            }
            Ok(Command::OpenMenu {
                target: fields.open_menu,
                until: fields.until,
                message: fields.message,
                open_states,
            })
        }
        "activate" => {
            let fields: ActivateFields = serde_json::from_value(fields).map_err(invalid)?;
            if !matches!(
                fields.activate,
                Target::Role { .. } | Target::DialogButton(_)
            ) {
                return Err("activate takes a role target or a dialog_button".to_owned());
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
            if matches!(fields.remember, Target::Field(Some(_))) {
                return Err(
                    "remember keeps the focused field, {\"field\":true} with an optional \
                     within, or a box"
                        .to_owned(),
                );
            }
            if fields.name.is_empty() {
                return Err("remember needs a name in as".to_owned());
            }
            Ok(Command::Remember {
                target: fields.remember,
                name: fields.name,
            })
        }
        "note" => {
            let fields: NoteFields = serde_json::from_value(fields).map_err(invalid)?;
            if fields.name.is_empty() {
                return Err("note needs a name in as".to_owned());
            }
            if fields.dir.is_some() && !fields.note.takes_directory() {
                return Err(format!(
                    "dir does not apply to note {:?}",
                    fields.note.name()
                ));
            }
            if (fields.from.is_some() || fields.through.is_some()) && !fields.note.is_list() {
                return Err("from and through slice a list".to_owned());
            }
            if fields.unless_first.is_some() != (fields.note == Accessor::MiddleVisible) {
                return Err("unless_first belongs to, and is needed by, middle_visible".to_owned());
            }
            if !fields.until_found && (fields.timeout_ms.is_some() || fields.message.is_some()) {
                return Err("timeout_ms and message apply to a note with until_found".to_owned());
            }
            if fields.of.is_some()
                != matches!(fields.note, Accessor::ScrollTarget | Accessor::Bounds)
            {
                return Err("of belongs to, and is needed by, scroll_target and bounds".to_owned());
            }
            if (fields.middle.is_some()
                || !fields.plus.is_empty()
                || fields.sorted
                || fields.pick.is_some())
                && !fields.note.is_list()
            {
                return Err("middle, plus, sorted, and pick take a list".to_owned());
            }
            if fields.expr.is_some() != (fields.note == Accessor::Number) {
                return Err("expr belongs to, and is needed by, number".to_owned());
            }
            if !fields.until.is_empty() && !(fields.note == Accessor::Number && fields.until_found)
            {
                return Err("until bounds a number noted with until_found".to_owned());
            }
            if fields.until_names.is_some() && !(fields.note.is_list() && fields.until_found) {
                return Err("until_names bounds a list noted with until_found".to_owned());
            }
            Ok(Command::Note {
                value: fields.note,
                dir: fields.dir,
                wait: fields.until_found.then(|| {
                    fields
                        .timeout_ms
                        .map_or(DEFAULT_WAIT, Duration::from_millis)
                }),
                message: fields.message,
                from: fields.from,
                through: fields.through,
                unless_first: fields.unless_first,
                of: fields.of,
                middle: fields.middle,
                plus: fields.plus,
                sorted: fields.sorted,
                pick: fields.pick,
                expr: fields.expr,
                until: fields.until,
                until_names: fields.until_names,
            })
        }
        "click" => {
            let fields: ClickFields = serde_json::from_value(fields).map_err(invalid)?;
            if !matches!(fields.button, 1 | 3) || !matches!(fields.count, 1 | 2) {
                return Err("click takes button 1 or 3 and count 1 or 2".to_owned());
            }
            if fields.release_mods_first && (fields.count != 1 || fields.mods.is_empty()) {
                return Err("release_mods_first takes one click with mods".to_owned());
            }
            fields.click.accepts(fields.at.as_ref())?;
            Ok(Command::Click {
                target: fields.click,
                button: fields.button,
                count: fields.count,
                modifiers: modifiers(&fields.mods)?,
                at: fields.at,
                left_bound: fields.left_bound,
                release_modifiers_first: fields.release_mods_first,
            })
        }
        "click_result_rows" => {
            let fields: ClickResultRowsFields = serde_json::from_value(fields).map_err(invalid)?;
            if fields.click_result_rows == RowStates::default() {
                return Err("click_result_rows needs a state to match".to_owned());
            }
            Ok(Command::ClickResultRows {
                states: fields.click_result_rows,
                modifiers: modifiers(&fields.mods)?,
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
                left_bound: fields.left_bound,
            })
        }
        "move_by" => {
            let fields: MoveByFields = serde_json::from_value(fields).map_err(invalid)?;
            let [x, y] = fields.move_by;
            Ok(Command::MoveBy { x, y })
        }
        "button_down" => {
            let fields: ButtonDownFields = serde_json::from_value(fields).map_err(invalid)?;
            if !matches!(fields.button_down, 1..=3) {
                return Err("button_down takes button 1, 2, or 3".to_owned());
            }
            Ok(Command::ButtonDown(fields.button_down))
        }
        "set_scrollbar" => {
            let fields: SetScrollbarFields = serde_json::from_value(fields).map_err(invalid)?;
            Ok(Command::SetScrollbar {
                orientation: fields.set_scrollbar,
                value: fields.value,
            })
        }
        "sleep" => {
            let fields: SleepFields = serde_json::from_value(fields).map_err(invalid)?;
            Ok(Command::Sleep(Duration::from_millis(fields.sleep)))
        }
        "wheel" => {
            let fields: WheelFields = serde_json::from_value(fields).map_err(invalid)?;
            if fields.clicks == 0 {
                return Err("wheel needs at least one click".to_owned());
            }
            fields.wheel.accepts(fields.at.as_ref())?;
            if fields.until.is_none()
                && (fields.tries.is_some()
                    || fields.timeout_ms.is_some()
                    || fields.message.is_some())
            {
                return Err("tries, timeout_ms, and message belong to a wheel's until".to_owned());
            }
            if fields.tries.is_some() && (fields.timeout_ms.is_some() || fields.message.is_some()) {
                return Err(
                    "a wheel repeats up to tries times or until a timeout, not both".to_owned(),
                );
            }
            if fields.tries == Some(0) {
                return Err("tries must be at least one".to_owned());
            }
            Ok(Command::Wheel {
                target: fields.wheel,
                at: fields.at,
                clicks: fields.clicks,
                down: matches!(fields.direction, WheelDirection::Down),
                modifiers: modifiers(&fields.mods)?,
                until: fields.until.map(|predicate| Repeat {
                    predicate,
                    tries: fields.tries,
                    timeout: fields
                        .timeout_ms
                        .map_or(DEFAULT_WAIT, Duration::from_millis),
                    message: fields.message,
                }),
            })
        }
        "drag" => {
            let fields: DragFields = serde_json::from_value(fields).map_err(invalid)?;
            if fields.steps == 0 {
                return Err("drag needs at least one step".to_owned());
            }
            fields.drag.from.accepts(fields.drag.from_at.as_ref())?;
            fields.drag.to.accepts(fields.drag.to_at.as_ref())?;
            Ok(Command::Drag {
                from: fields.drag.from,
                to: fields.drag.to,
                from_at: fields.drag.from_at,
                to_at: fields.drag.to_at,
                steps: fields.steps,
                release: fields.release,
                modifiers: modifiers(&fields.mods)?,
                after_press: press_checks(fields.after_press, fields.message)?,
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
            if fields.resize.width == 0 || fields.resize.height == Some(0) {
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
                name: fields.name,
            })
        }
        "wait" => {
            let fields: WaitFields = serde_json::from_value(fields).map_err(invalid)?;
            if fields.samples == 0 {
                return Err("a wait needs at least one sample".to_owned());
            }
            if fields.never.is_none() && fields.never_message.is_some() {
                return Err("never_message belongs to never".to_owned());
            }
            Ok(Command::Wait {
                predicate: fields.wait,
                timeout: fields
                    .timeout_ms
                    .map_or(DEFAULT_WAIT, Duration::from_millis),
                message: fields.message,
                samples: fields.samples,
                settle: fields.settling,
                never: fields
                    .never
                    .map(|predicate| Box::new((predicate, fields.never_message))),
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
            if fields.hold_ms.is_some() && fields.timeout_ms.is_none() {
                return Err("hold_ms belongs to an expect with timeout_ms".to_owned());
            }
            Ok(Command::Expect {
                predicates,
                message: fields.message,
                timeout: fields.timeout_ms.map(Duration::from_millis),
                hold: fields.hold_ms.map(Duration::from_millis),
            })
        }
        _ => {
            let fields: QueryFields = serde_json::from_value(fields).map_err(invalid)?;
            Ok(Command::Query(fields.query))
        }
    }
}

fn press_checks(
    after_press: Option<Value>,
    message: Option<String>,
) -> Result<Vec<PressCheck>, String> {
    let invalid = |error: serde_json::Error| format!("invalid after_press: {error}");
    match after_press {
        None if message.is_some() => Err("a drag's message names its after_press check".to_owned()),
        None => Ok(Vec::new()),
        Some(Value::Array(steps)) => {
            if message.is_some() {
                return Err("each after_press step carries its own message".to_owned());
            }
            if steps.is_empty() {
                return Err("after_press needs a step".to_owned());
            }
            steps
                .into_iter()
                .map(|step| {
                    let fields: PressStepFields = serde_json::from_value(step).map_err(invalid)?;
                    match (fields.wait, fields.expect) {
                        (Some(predicate), None) => Ok(PressCheck {
                            predicate,
                            wait: Some(fields.timeout_ms.map_or(DEFAULT_WAIT, Duration::from_millis)),
                            message: fields.message,
                        }),
                        (None, Some(predicate)) if fields.timeout_ms.is_none() => Ok(PressCheck {
                            predicate,
                            wait: None,
                            message: fields.message,
                        }),
                        _ => Err(
                            "an after_press step is a wait (with an optional timeout_ms) or an expect"
                                .to_owned(),
                        ),
                    }
                })
                .collect()
        }
        Some(predicate) => Ok(vec![PressCheck {
            predicate: serde_json::from_value(predicate).map_err(invalid)?,
            wait: None,
            message,
        }]),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct At {
    pub(crate) x: Offset,
    pub(crate) y: Offset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Offset {
    Pixels(i32),
    Percent(u8),
    Fraction(u16, u16),
    FromStart(i32),
    FractionPlus(u16, u16, i32),
    FromEnd(i32),
    EndLessFraction(u16, u16),
}

impl Offset {
    pub(crate) fn parse(value: &Value) -> Result<Self, String> {
        if let Some(pixels) = value.as_i64() {
            return i32::try_from(pixels)
                .map(Self::Pixels)
                .map_err(|_| format!("{pixels} is out of range"));
        }
        let text = value.as_str().unwrap_or_default();
        let percent = text
            .strip_suffix('%')
            .and_then(|number| number.parse::<u8>().ok())
            .filter(|percent| *percent <= 100)
            .map(Self::Percent);
        let fraction = |text: &str| {
            text.split_once('/')
                .and_then(|(numerator, denominator)| {
                    Some((
                        numerator.parse::<u16>().ok()?,
                        denominator.parse::<u16>().ok()?,
                    ))
                })
                .filter(|(numerator, denominator)| *denominator > 0 && numerator <= denominator)
        };
        let share =
            fraction(text).map(|(numerator, denominator)| Self::Fraction(numerator, denominator));
        let share_plus = text
            .rfind(['+', '-'])
            .filter(|position| *position > 0)
            .and_then(|position| {
                let (numerator, denominator) = fraction(&text[..position])?;
                let pixels = text[position..].parse::<i32>().ok()?;
                Some(Self::FractionPlus(numerator, denominator, pixels))
            });
        let from_start = text
            .strip_prefix("start")
            .filter(|offset| offset.starts_with(['+', '-']))
            .and_then(|offset| offset.parse::<i32>().ok())
            .map(Self::FromStart);
        let from_end = text.strip_prefix("end").and_then(|offset| {
            if let Some((numerator, denominator)) = offset.strip_prefix('-').and_then(fraction) {
                return Some(Self::EndLessFraction(numerator, denominator));
            }
            offset
                .starts_with(['+', '-'])
                .then(|| offset.parse::<i32>().ok())
                .flatten()
                .map(Self::FromEnd)
        });
        percent
            .or(share)
            .or(share_plus)
            .or(from_start)
            .or(from_end)
            .ok_or_else(|| {
                format!(
                    "an offset is whole pixels, a share like \"80%\", \"2/3\", or \"2/3+3\", \
                     \"start-6\", \"end+60\", or \"end-1/6\", not {value}"
                )
            })
    }

    pub(crate) fn resolve(self, start: f64, extent: f64) -> f64 {
        match self {
            Self::Pixels(pixels) if pixels < 0 => start + extent + f64::from(pixels),
            Self::Pixels(pixels) | Self::FromStart(pixels) => start + f64::from(pixels),
            Self::Percent(percent) => start + (extent * f64::from(percent) / 100.0).floor(),
            Self::Fraction(numerator, denominator) => {
                start + (extent * f64::from(numerator) / f64::from(denominator)).floor()
            }
            Self::FractionPlus(numerator, denominator, pixels) => {
                Self::Fraction(numerator, denominator).resolve(start, extent) + f64::from(pixels)
            }
            Self::FromEnd(pixels) => start + extent + f64::from(pixels),
            Self::EndLessFraction(numerator, denominator) => {
                start + extent - (extent * f64::from(numerator) / f64::from(denominator)).floor()
            }
        }
    }
}

impl Offset {
    fn to_json(self) -> Value {
        match self {
            Self::Pixels(pixels) => json!(pixels),
            Self::Percent(percent) => json!(format!("{percent}%")),
            Self::Fraction(numerator, denominator) => json!(format!("{numerator}/{denominator}")),
            Self::FromStart(pixels) => json!(format!("start{pixels:+}")),
            Self::FractionPlus(numerator, denominator, pixels) => {
                json!(format!("{numerator}/{denominator}{pixels:+}"))
            }
            Self::FromEnd(pixels) => json!(format!("end{pixels:+}")),
            Self::EndLessFraction(numerator, denominator) => {
                json!(format!("end-{numerator}/{denominator}"))
            }
        }
    }
}

impl Serialize for At {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        json!([self.x.to_json(), self.y.to_json()]).serialize(serializer)
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

pub(crate) const TARGET_KINDS: [&str; 27] = [
    "entry",
    "row",
    "result",
    "result_row",
    "focused_widget",
    "remembered",
    "search_result",
    "role",
    "named",
    "background",
    "pane",
    "viewport",
    "container",
    "field",
    "sidebar",
    "header",
    "menu_item",
    "dialog_button",
    "dialog",
    "window",
    "preview",
    "parent",
    "ancestor",
    "kept",
    "point",
    "toplevel",
    "other_window",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Entry(EntryTarget),
    Role {
        role: String,
        other_roles: Vec<String>,
        name: Option<String>,
        description: Option<String>,
        within: Option<Box<Target>>,
        name_ends_with: Vec<String>,
        nth: Option<i64>,
        states: Vec<StateName>,
        name_matches: Option<String>,
        has: Option<Box<Target>>,
        rendered: bool,
    },
    Row(String),
    Background {
        dir: Option<String>,
    },
    Pane {
        dir: Option<String>,
    },
    Viewport {
        dir: Option<String>,
    },
    Field(Option<String>),
    FieldWithin(Box<Target>),
    Toplevel(String),
    OtherWindow(String),
    Result(String),
    ResultRow {
        index: i64,
        containing: Option<String>,
        name: Option<String>,
    },
    FocusedWidget,
    Remembered(String),
    Sidebar(String),
    Header(String),
    MenuItem(String),
    DialogButton(String),
    Window {
        y_of: Option<Box<Target>>,
    },
    Preview,
    SearchResult(String),
    Named {
        name: String,
        within: Box<Target>,
    },
    Container {
        dir: Option<String>,
    },
    Dialog,
    Parent(Box<Target>),
    Ancestor {
        role: String,
        of: Box<Target>,
    },
    Kept(String),
    Point(Box<[Axis; 2]>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Axis {
    Of {
        target: Box<Target>,
        at: Option<Offset>,
        plus: i32,
    },
    Mid {
        pair: Box<[Axis; 2]>,
        plus: i32,
    },
    Max {
        pair: Box<[Axis; 2]>,
        plus: i32,
    },
    Extent {
        target: Box<Target>,
        plus: i32,
    },
}

impl Axis {
    fn from_value(value: &Value) -> Result<Self, String> {
        let Value::Object(object) = value else {
            return Err(format!("a point coordinate is an object, not {value}"));
        };
        let plus = match object.get("plus") {
            None => 0,
            Some(plus) => plus
                .as_i64()
                .and_then(|plus| i32::try_from(plus).ok())
                .ok_or_else(|| format!("plus is whole pixels, not {plus}"))?,
        };
        let pair = |value: &Value| -> Result<Box<[Axis; 2]>, String> {
            match value.as_array().map(Vec::as_slice) {
                Some([first, second]) => Ok(Box::new([
                    Self::from_value(first)?,
                    Self::from_value(second)?,
                ])),
                _ => Err(format!("mid and max take two coordinates, not {value}")),
            }
        };
        let known = ["of", "at", "plus", "mid", "max", "extent"];
        if let Some(key) = object.keys().find(|key| !known.contains(&key.as_str())) {
            return Err(format!("a point coordinate has no field {key:?}"));
        }
        if let Some(target) = object.get("extent") {
            if ["of", "at", "mid", "max"]
                .iter()
                .any(|key| object.contains_key(*key))
            {
                return Err("extent takes only a target and plus".to_owned());
            }
            return Ok(Self::Extent {
                target: Box::new(
                    serde_json::from_value(target.clone()).map_err(|error| error.to_string())?,
                ),
                plus,
            });
        }
        match (object.get("of"), object.get("mid"), object.get("max")) {
            (Some(target), None, None) => Ok(Self::Of {
                target: Box::new(
                    serde_json::from_value(target.clone()).map_err(|error| error.to_string())?,
                ),
                at: object.get("at").map(Offset::parse).transpose()?,
                plus,
            }),
            (None, Some(both), None) if !object.contains_key("at") => Ok(Self::Mid {
                pair: pair(both)?,
                plus,
            }),
            (None, None, Some(both)) if !object.contains_key("at") => Ok(Self::Max {
                pair: pair(both)?,
                plus,
            }),
            _ => Err(
                "a point coordinate is {\"of\":TARGET,\"at\":OFFSET}, {\"mid\":[A,B]}, or \
                 {\"max\":[A,B]}, each with an optional plus"
                    .to_owned(),
            ),
        }
    }

    fn to_json(&self) -> Value {
        let (mut object, plus) = match self {
            Self::Of { target, at, plus } => {
                let mut object = Map::new();
                object.insert("of".to_owned(), target.to_json());
                if let Some(at) = at {
                    object.insert("at".to_owned(), at.to_json());
                }
                (object, *plus)
            }
            Self::Mid { pair, plus } | Self::Max { pair, plus } => {
                let key = if matches!(self, Self::Mid { .. }) {
                    "mid"
                } else {
                    "max"
                };
                let mut object = Map::new();
                object.insert(
                    key.to_owned(),
                    json!([pair[0].to_json(), pair[1].to_json()]),
                );
                (object, *plus)
            }
            Self::Extent { target, plus } => {
                let mut object = Map::new();
                object.insert("extent".to_owned(), target.to_json());
                (object, *plus)
            }
        };
        if plus != 0 {
            object.insert("plus".to_owned(), json!(plus));
        }
        Value::Object(object)
    }
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
            role: Value,
            name: Option<String>,
            description: Option<String>,
            within: Option<Target>,
            #[serde(default)]
            name_ends_with: Vec<String>,
            nth: Option<i64>,
            #[serde(default)]
            states: Vec<StateName>,
            name_matches: Option<String>,
            has: Option<Target>,
            #[serde(default = "yes")]
            rendered: bool,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct BackgroundFields {
            background: bool,
            dir: Option<String>,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WindowFields {
            window: bool,
            y_of: Option<Target>,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct PreviewFields {
            preview: bool,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct DialogFields {
            dialog: bool,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct NamedFields {
            named: String,
            within: Target,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct ResultRowFields {
            result_row: i64,
            containing: Option<String>,
            name: Option<String>,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct FocusedWidgetFields {
            focused_widget: bool,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct AncestorFields {
            ancestor: String,
            of: Target,
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
                let roles: Vec<String> = match &fields.role {
                    Value::String(role) => vec![role.clone()],
                    Value::Array(_) => {
                        serde_json::from_value(fields.role.clone()).map_err(invalid)?
                    }
                    _ => Vec::new(),
                };
                let Some((role, other_roles)) = roles.split_first() else {
                    return Err(
                        "invalid role target: role is a role name or a list of them".to_owned()
                    );
                };
                if let Some(pattern) = &fields.name_matches {
                    regex::Regex::new(pattern).map_err(|error| {
                        format!("invalid role target: name_matches {pattern:?}: {error}")
                    })?;
                }
                if let Some(has) = &fields.has
                    && !matches!(
                        has,
                        Self::Role {
                            within: None,
                            has: None,
                            rendered: true,
                            ..
                        }
                    )
                {
                    return Err(
                        "invalid role target: has takes a rendered role target without within \
                         or has"
                            .to_owned(),
                    );
                }
                if role == "*" && !other_roles.is_empty() {
                    return Err("invalid role target: \"*\" stands alone".to_owned());
                }
                Ok(Self::Role {
                    role: role.clone(),
                    other_roles: other_roles.to_vec(),
                    name: fields.name,
                    description: fields.description,
                    within: fields.within.map(Box::new),
                    name_ends_with: fields.name_ends_with,
                    nth: fields.nth,
                    states: fields.states,
                    name_matches: fields.name_matches,
                    has: fields.has.map(Box::new),
                    rendered: fields.rendered,
                })
            }
            "background" => {
                let fields: BackgroundFields = serde_json::from_value(value).map_err(invalid)?;
                if !fields.background {
                    return Err("invalid background target: background must be true".to_owned());
                }
                Ok(Self::Background { dir: fields.dir })
            }
            "field" => {
                let object = value.as_object().cloned().unwrap_or_default();
                match (object.get("field"), object.get("within"), object.len()) {
                    (Some(Value::Bool(true)), None, 1) => Ok(Self::Field(None)),
                    (Some(Value::Bool(true)), Some(within), 2) => {
                        serde_json::from_value(within.clone())
                            .map(|within| Self::FieldWithin(Box::new(within)))
                            .map_err(invalid)
                    }
                    (Some(Value::String(name)), None, 1) if !name.is_empty() => {
                        Ok(Self::Field(Some(name.clone())))
                    }
                    _ => Err(
                        "invalid field target: field is true, with an optional within, \
                         or the name a remember gave it"
                            .to_owned(),
                    ),
                }
            }
            "toplevel" => label(value).map(Self::Toplevel),
            "row" => label(value).map(Self::Row),
            "other_window" => label(value).map(Self::OtherWindow),
            "result" => label(value).map(Self::Result),
            "result_row" => {
                let fields: ResultRowFields = serde_json::from_value(value).map_err(invalid)?;
                if fields.containing.is_some() && fields.name.is_some() {
                    return Err(
                        "invalid result_row target: containing or name, not both".to_owned()
                    );
                }
                Ok(Self::ResultRow {
                    index: fields.result_row,
                    containing: fields.containing,
                    name: fields.name,
                })
            }
            "focused_widget" => {
                let fields: FocusedWidgetFields = serde_json::from_value(value).map_err(invalid)?;
                if !fields.focused_widget {
                    return Err(
                        "invalid focused_widget target: focused_widget must be true".to_owned()
                    );
                }
                Ok(Self::FocusedWidget)
            }
            "remembered" => label(value).map(Self::Remembered),
            "pane" => directory(value).map(|dir| Self::Pane { dir }),
            "viewport" => directory(value).map(|dir| Self::Viewport { dir }),
            "sidebar" => label(value).map(Self::Sidebar),
            "header" => label(value).map(Self::Header),
            "menu_item" => label(value).map(Self::MenuItem),
            "window" => {
                let fields: WindowFields = serde_json::from_value(value).map_err(invalid)?;
                if !fields.window {
                    return Err("invalid window target: window must be true".to_owned());
                }
                Ok(Self::Window {
                    y_of: fields.y_of.map(Box::new),
                })
            }
            "preview" => {
                let fields: PreviewFields = serde_json::from_value(value).map_err(invalid)?;
                if !fields.preview {
                    return Err("invalid preview target: preview must be true".to_owned());
                }
                Ok(Self::Preview)
            }
            "dialog" => {
                let fields: DialogFields = serde_json::from_value(value).map_err(invalid)?;
                if !fields.dialog {
                    return Err("invalid dialog target: dialog must be true".to_owned());
                }
                Ok(Self::Dialog)
            }
            "search_result" => label(value).map(Self::SearchResult),
            "named" => {
                let fields: NamedFields = serde_json::from_value(value).map_err(invalid)?;
                Ok(Self::Named {
                    name: fields.named,
                    within: Box::new(fields.within),
                })
            }
            "container" => directory(value).map(|dir| Self::Container { dir }),
            "parent" => {
                let object = value.as_object().cloned().unwrap_or_default();
                if object.len() != 1 {
                    return Err("invalid parent target: it takes only parent".to_owned());
                }
                serde_json::from_value(object["parent"].clone())
                    .map(|of| Self::Parent(Box::new(of)))
                    .map_err(invalid)
            }
            "ancestor" => {
                let fields: AncestorFields = serde_json::from_value(value).map_err(invalid)?;
                Ok(Self::Ancestor {
                    role: fields.ancestor,
                    of: Box::new(fields.of),
                })
            }
            "kept" => label(value).map(Self::Kept),
            "point" => {
                let object = value.as_object().cloned().unwrap_or_default();
                if object.len() != 1 {
                    return Err("invalid point target: it takes only point".to_owned());
                }
                match object["point"].as_array().map(Vec::as_slice) {
                    Some([x, y]) => Ok(Self::Point(Box::new([
                        Axis::from_value(x)
                            .map_err(|error| format!("invalid point target: x: {error}"))?,
                        Axis::from_value(y)
                            .map_err(|error| format!("invalid point target: y: {error}"))?,
                    ]))),
                    _ => Err("invalid point target: point is [x, y]".to_owned()),
                }
            }
            _ => label(value).map(Self::DialogButton),
        }
    }

    pub(crate) fn to_json(&self) -> Value {
        match self {
            Self::Entry(entry) => json!(entry),
            Self::Role {
                role,
                other_roles,
                name,
                description,
                within,
                name_ends_with,
                nth,
                states,
                name_matches,
                has,
                rendered,
            } => {
                let mut object = Map::new();
                if let Some(has) = has {
                    object.insert("has".to_owned(), has.to_json());
                }
                if !rendered {
                    object.insert("rendered".to_owned(), json!(false));
                }
                if other_roles.is_empty() {
                    object.insert("role".to_owned(), json!(role));
                } else {
                    let mut roles = vec![role.clone()];
                    roles.extend(other_roles.iter().cloned());
                    object.insert("role".to_owned(), json!(roles));
                }
                if !states.is_empty() {
                    object.insert("states".to_owned(), json!(states));
                }
                if let Some(pattern) = name_matches {
                    object.insert("name_matches".to_owned(), json!(pattern));
                }
                if let Some(name) = name {
                    object.insert("name".to_owned(), json!(name));
                }
                if let Some(description) = description {
                    object.insert("description".to_owned(), json!(description));
                }
                if let Some(within) = within {
                    object.insert("within".to_owned(), within.to_json());
                }
                if !name_ends_with.is_empty() {
                    object.insert("name_ends_with".to_owned(), json!(name_ends_with));
                }
                if let Some(nth) = nth {
                    object.insert("nth".to_owned(), json!(nth));
                }
                Value::Object(object)
            }
            Self::Background { dir: None } => json!({ "background": true }),
            Self::Background { dir } => json!({ "background": true, "dir": dir }),
            Self::Pane { dir } => json!({ "pane": dir }),
            Self::Viewport { dir } => json!({ "viewport": dir }),
            Self::Field(None) => json!({ "field": true }),
            Self::Field(Some(name)) => json!({ "field": name }),
            Self::FieldWithin(within) => json!({ "field": true, "within": within.to_json() }),
            Self::Toplevel(title) => json!({ "toplevel": title }),
            Self::Row(name) => json!({ "row": name }),
            Self::OtherWindow(title) => json!({ "other_window": title }),
            Self::Result(path) => json!({ "result": path }),
            Self::ResultRow {
                index,
                containing,
                name,
            } => {
                let mut object = Map::new();
                object.insert("result_row".to_owned(), json!(index));
                if let Some(containing) = containing {
                    object.insert("containing".to_owned(), json!(containing));
                }
                if let Some(name) = name {
                    object.insert("name".to_owned(), json!(name));
                }
                Value::Object(object)
            }
            Self::FocusedWidget => json!({ "focused_widget": true }),
            Self::Remembered(name) => json!({ "remembered": name }),
            Self::Sidebar(name) => json!({ "sidebar": name }),
            Self::Header(name) => json!({ "header": name }),
            Self::MenuItem(name) => json!({ "menu_item": name }),
            Self::DialogButton(name) => json!({ "dialog_button": name }),
            Self::Window { y_of: None } => json!({ "window": true }),
            Self::Window { y_of: Some(target) } => {
                json!({ "window": true, "y_of": target.to_json() })
            }
            Self::Preview => json!({ "preview": true }),
            Self::SearchResult(name) => json!({ "search_result": name }),
            Self::Named { name, within } => json!({ "named": name, "within": within.to_json() }),
            Self::Container { dir } => json!({ "container": dir }),
            Self::Dialog => json!({ "dialog": true }),
            Self::Parent(of) => json!({ "parent": of.to_json() }),
            Self::Ancestor { role, of } => json!({ "ancestor": role, "of": of.to_json() }),
            Self::Kept(name) => json!({ "kept": name }),
            Self::Point(axes) => json!({ "point": [axes[0].to_json(), axes[1].to_json()] }),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StateName {
    Selected,
    Focused,
    Checked,
    Pressed,
    Sensitive,
    Selectable,
    Editable,
    Horizontal,
    Vertical,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EntryTarget {
    pub(crate) entry: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) part: Option<EntryPart>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EntryPart {
    Label,
    Icon,
    Metadata,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Accessor {
    Directory,
    Pane,
    Panes,
    Entries,
    Selected,
    AllSelected,
    Matches,
    Focused,
    FocusedPane,
    MiddleVisible,
    Visible,
    ScrollTarget,
    Bounds,
    EditableFields,
    MenuItems,
    Number,
    VisibleTop,
}

impl Accessor {
    pub(crate) fn name(self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default()
    }

    pub(crate) fn takes_directory(self) -> bool {
        matches!(
            self,
            Self::Entries | Self::Selected | Self::Matches | Self::Visible | Self::ScrollTarget
        )
    }

    pub(crate) fn is_list(self) -> bool {
        matches!(
            self,
            Self::Panes
                | Self::Entries
                | Self::Selected
                | Self::AllSelected
                | Self::Matches
                | Self::Visible
                | Self::MenuItems
                | Self::VisibleTop
        )
    }
}

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
    Panes(Names),
    Pane(Option<String>),
    Entries(Names),
    Entry(String),
    EntryGone(String),
    Selected(Names),
    AllSelected(Names),
    Focused(Presence),
    FocusedInMatches(bool),
    FocusedPane(Presence),
    ViewMode(String),
    WindowWidth(i32),
    Dialog(Presence),
    MenuItems(Option<Names>),
    Preview(Presence),
    Peek(bool),
    Matches(Names),
    Field(FieldCheck),
    Pref(PrefCheck),
    EditorWithinViewport(FieldRef),
    State(StateCheck),
    Any(Vec<Predicate>),
    Present(Target),
    Absent(Target),
    Moved(Movement),
    OnScreen(Target),
    Blank(BlankCheck),
    Not(Box<Predicate>),
    Visible(Names),
    ScrollValue(ScrollCheck),
    BandSelected(BandCheck),
    FocusWithin(Target),
    Resized(Resized),
    ContainsPoint(ContainsPoint),
    FooterFill(FooterFill),
    Repainted(Repainted),
    ContrastBelow(ContrastBelow),
    Elapsed(Elapsed),
    DiagnosticsMention(Vec<String>),
    All(Vec<Predicate>),
    EditableFields(usize),
    Description(DescriptionCheck),
    Mentions(MentionCheck),
    Presented(Target),
    File(FileCheck),
    FocusedWidget(Option<TextMatch>),
    Inside(InsideCheck),
    EditorWithinWindow(EditorWindowCheck),
    Replaced(Replaced),
    Pixel(PixelCheck),
    AccentPixels(AccentPixels),
    AnyResultRow(RowStates),
    HitPath(HitPath),
    Compare(Comparison),
    PanesWith(PanesWith),
    Name(NameCheck),
    EditableText(String),
    VisibleTop(Names),
    LargestImage(LargestImage),
    Children(ChildrenCheck),
    ListSections(SectionsCheck),
    Text(NameCheck),
    SomeWindow(SomeWindow),
    Windows(WindowCount),
    ChildNames(ChildNames),
    FocusOutside(Target),
    FocusedWidgetIs(Target),
    Alive(String),
    SubtreeText(SubtreeText),
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LargestImage {
    pub(crate) within: Target,
    #[serde(default)]
    pub(crate) wider_than: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) size: Option<[i64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) exceeds: Option<[i64; 2]>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChildrenCheck {
    pub(crate) comm_prefix: String,
    pub(crate) count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SectionsCheck {
    pub(crate) within: Target,
    pub(crate) section: String,
    pub(crate) names: Names,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Bound {
    pub(crate) op: CompareOp,
    pub(crate) than: Expr,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Comparison {
    pub(crate) left: Expr,
    pub(crate) op: CompareOp,
    pub(crate) right: Expr,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub(crate) enum CompareOp {
    #[serde(rename = "<")]
    Less,
    #[serde(rename = "<=")]
    AtMost,
    #[serde(rename = "==")]
    Equal,
    #[serde(rename = "!=")]
    Unequal,
    #[serde(rename = ">=")]
    AtLeast,
    #[serde(rename = ">")]
    Greater,
}

impl CompareOp {
    pub(crate) fn holds(self, left: f64, right: f64) -> bool {
        match self {
            Self::Less => left < right,
            Self::AtMost => left <= right,
            #[expect(clippy::float_cmp, reason = "Python compares the numbers exactly")]
            Self::Equal => left == right,
            #[expect(clippy::float_cmp, reason = "Python compares the numbers exactly")]
            Self::Unequal => left != right,
            Self::AtLeast => left >= right,
            Self::Greater => left > right,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Expr {
    Number(serde_json::Number),
    Of {
        target: Box<Target>,
        get: Measure,
    },
    Pref {
        key: String,
        default: Option<serde_json::Number>,
    },
    Count(Box<CountSpec>),
    Sum(Vec<Expr>),
    Minus(Box<[Expr; 2]>),
    Times(Box<[Expr; 2]>),
    Divide(Box<[Expr; 2]>),
    FloorDivide(Box<[Expr; 2]>),
    Ceil(Box<Expr>),
    Abs(Box<Expr>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Measure {
    X,
    Y,
    Width,
    Height,
    Right,
    Bottom,
    CenterX,
    CenterY,
    Value,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CountSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) within: Option<Target>,
    #[serde(default = "yes")]
    pub(crate) rendered: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) states: Vec<StateName>,
}

impl Expr {
    fn from_value(value: &Value) -> Result<Self, String> {
        if let Value::Number(number) = value {
            return Ok(Self::Number(number.clone()));
        }
        let Value::Object(object) = value else {
            return Err(format!("a number is a literal or an object, not {value}"));
        };
        let pair = |value: &Value| -> Result<Box<[Expr; 2]>, String> {
            match value.as_array().map(Vec::as_slice) {
                Some([first, second]) => Ok(Box::new([
                    Self::from_value(first)?,
                    Self::from_value(second)?,
                ])),
                _ => Err(format!("an operation takes two numbers, not {value}")),
            }
        };
        let only = |keys: &[&str]| -> Result<(), String> {
            match object.keys().find(|key| !keys.contains(&key.as_str())) {
                Some(key) => Err(format!("a number has no field {key:?} here")),
                None => Ok(()),
            }
        };
        let target = |value: &Value| -> Result<Box<Target>, String> {
            serde_json::from_value(value.clone())
                .map(Box::new)
                .map_err(|error| error.to_string())
        };
        if let Some(of) = object.get("of") {
            only(&["of", "get"])?;
            let get = object.get("get").ok_or("of needs get, such as \"width\"")?;
            return Ok(Self::Of {
                target: target(of)?,
                get: serde_json::from_value(get.clone()).map_err(|error| error.to_string())?,
            });
        }
        if let Some(key) = object.get("pref") {
            only(&["pref", "default"])?;
            return Ok(Self::Pref {
                key: key.as_str().ok_or("pref names a key")?.to_owned(),
                default: match object.get("default") {
                    None => None,
                    Some(Value::Number(number)) => Some(number.clone()),
                    Some(other) => return Err(format!("default is a number, not {other}")),
                },
            });
        }
        let [(key, inner)] = object.iter().collect::<Vec<_>>()[..] else {
            return Err(format!("a number has one operation, not {value}"));
        };
        Ok(match key.as_str() {
            "count" => Self::Count(Box::new(
                serde_json::from_value(inner.clone()).map_err(|error| error.to_string())?,
            )),
            "sum" => Self::Sum(
                inner
                    .as_array()
                    .ok_or("sum takes a list")?
                    .iter()
                    .map(Self::from_value)
                    .collect::<Result<_, _>>()?,
            ),
            "minus" => Self::Minus(pair(inner)?),
            "times" => Self::Times(pair(inner)?),
            "divide" => Self::Divide(pair(inner)?),
            "floor_divide" => Self::FloorDivide(pair(inner)?),
            "ceil" => Self::Ceil(Box::new(Self::from_value(inner)?)),
            "abs" => Self::Abs(Box::new(Self::from_value(inner)?)),
            other => return Err(format!("no number operation {other:?}")),
        })
    }

    pub(crate) fn to_json(&self) -> Value {
        let pair = |items: &[Expr; 2]| json!([items[0].to_json(), items[1].to_json()]);
        match self {
            Self::Number(number) => Value::Number(number.clone()),
            Self::Of { target, get } => json!({ "of": target.to_json(), "get": get }),
            Self::Pref { key, default: None } => json!({ "pref": key }),
            Self::Pref {
                key,
                default: Some(default),
            } => json!({ "pref": key, "default": default }),
            Self::Count(spec) => json!({ "count": spec }),
            Self::Sum(items) => {
                json!({ "sum": items.iter().map(Self::to_json).collect::<Vec<_>>() })
            }
            Self::Minus(items) => json!({ "minus": pair(items) }),
            Self::Times(items) => json!({ "times": pair(items) }),
            Self::Divide(items) => json!({ "divide": pair(items) }),
            Self::FloorDivide(items) => json!({ "floor_divide": pair(items) }),
            Self::Ceil(inner) => json!({ "ceil": inner.to_json() }),
            Self::Abs(inner) => json!({ "abs": inner.to_json() }),
        }
    }
}

impl<'de> Deserialize<'de> for Expr {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::from_value(&Value::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl Serialize for Expr {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_json().serialize(serializer)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PanesWith {
    pub(crate) target: Target,
    pub(crate) names: Names,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NameCheck {
    pub(crate) target: Target,
    pub(crate) text: TextMatch,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub(crate) rendered: bool,
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes a reference"
)]
fn is_true(value: &bool) -> bool {
    *value
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PixelCheck {
    pub(crate) target: Target,
    pub(crate) rgb: [u8; 3],
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AccentPixels {
    pub(crate) target: Target,
    #[serde(default)]
    pub(crate) inset: u16,
    pub(crate) more_than: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HitPath {
    pub(crate) mark: String,
    pub(crate) hits: std::collections::BTreeMap<String, String>,
    pub(crate) name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Replaced {
    pub(crate) target: Target,
    pub(crate) since: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DescriptionCheck {
    pub(crate) target: Target,
    pub(crate) text: TextMatch,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MentionCheck {
    pub(crate) target: Target,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) words: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) equals: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FileCheck {
    pub(crate) path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) exists: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) mode_bits: Option<ModeBits>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModeBits {
    pub(crate) mask: String,
    pub(crate) set: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SomeWindow {
    pub(crate) title: String,
    pub(crate) holds: Box<Predicate>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WindowCount {
    pub(crate) title: String,
    pub(crate) count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChildNames {
    pub(crate) of: Target,
    pub(crate) role: String,
    pub(crate) names: Names,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SubtreeText {
    pub(crate) target: Target,
    pub(crate) contains: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) strip_newlines: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InsideCheck {
    pub(crate) target: Target,
    pub(crate) of: Target,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) axis: Option<PointAxis>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EditorWindowCheck {
    pub(crate) of: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) pane: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ScrollCheck {
    pub(crate) at_least: serde_json::Number,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BandCheck {
    pub(crate) from: String,
    pub(crate) to: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Resized {
    pub(crate) target: Target,
    pub(crate) since: String,
    pub(crate) width: Change,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub(crate) enum Change {
    Exactly(i64),
    Bounded {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max: Option<i64>,
    },
}

impl Change {
    pub(crate) fn matches(&self, change: i64) -> bool {
        match self {
            Self::Exactly(expected) => *expected == change,
            Self::Bounded { min, max } => {
                min.is_none_or(|min| change >= min) && max.is_none_or(|max| change <= max)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ContainsPoint {
    pub(crate) target: Target,
    pub(crate) point: Target,
    pub(crate) axis: PointAxis,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PointAxis {
    X,
    Y,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FooterFill {
    pub(crate) ends: Vec<String>,
    pub(crate) total: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Repainted {
    pub(crate) since: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ContrastBelow {
    pub(crate) of: String,
    pub(crate) to: String,
    pub(crate) share: serde_json::Number,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Elapsed {
    pub(crate) since: String,
    pub(crate) ms: u64,
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
                | Self::Visible(_)
                | Self::ScrollValue(_)
                | Self::BandSelected(_)
        )
    }

    pub(crate) fn is_live(&self) -> bool {
        match self {
            Self::Present(_)
            | Self::Absent(_)
            | Self::Moved(_)
            | Self::Field(_)
            | Self::Pref(_)
            | Self::EditorWithinViewport(_)
            | Self::State(_)
            | Self::OnScreen(_)
            | Self::Blank(_)
            | Self::Visible(_)
            | Self::ScrollValue(_)
            | Self::BandSelected(_)
            | Self::FocusWithin(_)
            | Self::Resized(_)
            | Self::ContainsPoint(_)
            | Self::FooterFill(_)
            | Self::Repainted(_)
            | Self::ContrastBelow(_)
            | Self::Elapsed(_)
            | Self::DiagnosticsMention(_)
            | Self::EditableFields(_)
            | Self::Description(_)
            | Self::Mentions(_)
            | Self::Presented(_)
            | Self::File(_)
            | Self::FocusedWidget(_)
            | Self::Inside(_)
            | Self::EditorWithinWindow(_)
            | Self::Replaced(_)
            | Self::Pixel(_)
            | Self::AccentPixels(_)
            | Self::HitPath(_)
            | Self::AnyResultRow(_)
            | Self::Compare(_)
            | Self::PanesWith(_)
            | Self::Name(_)
            | Self::EditableText(_)
            | Self::VisibleTop(_)
            | Self::LargestImage(_)
            | Self::Children(_)
            | Self::ListSections(_)
            | Self::Text(_)
            | Self::SomeWindow(_)
            | Self::Windows(_)
            | Self::ChildNames(_)
            | Self::FocusOutside(_)
            | Self::FocusedWidgetIs(_)
            | Self::Alive(_)
            | Self::SubtreeText(_) => true,
            Self::Any(predicates) | Self::All(predicates) => {
                predicates.iter().any(|predicate| predicate.check.is_live())
            }
            Self::Not(predicate) => predicate.check.is_live(),
            _ => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Presence {
    Absent,
    Any,
    Named(String),
    OneOf(Vec<String>),
}

impl Presence {
    pub(crate) fn matches(&self, actual: Option<&str>) -> bool {
        match self {
            Self::Absent => actual.is_none(),
            Self::Any => actual.is_some(),
            Self::Named(name) => actual == Some(name.as_str()),
            Self::OneOf(names) => {
                actual.is_some_and(|actual| names.iter().any(|name| name == actual))
            }
        }
    }
}

impl<'de> Deserialize<'de> for Presence {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct OneOf {
            one_of: Vec<String>,
        }
        match Value::deserialize(deserializer)? {
            Value::Null => Ok(Self::Absent),
            Value::Bool(true) => Ok(Self::Any),
            Value::String(name) => Ok(Self::Named(name)),
            Value::Object(object) => serde_json::from_value::<OneOf>(Value::Object(object))
                .map(|names| Self::OneOf(names.one_of))
                .map_err(serde::de::Error::custom),
            other => Err(serde::de::Error::custom(format!(
                "expected a name, true for any, null, or {{\"one_of\":[...]}}, not {other}"
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
            Self::OneOf(names) => json!({ "one_of": names }).serialize(serializer),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FieldRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) of: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FieldCheck {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) of: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) text: Option<TextMatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) caret: Option<CaretMatch>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) selection: Option<Option<[i32; 2]>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) focused: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) showing: Option<bool>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) lowercase_contains: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) longer_than: Option<usize>,
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
                    && pattern
                        .lowercase_contains
                        .as_deref()
                        .is_none_or(|part| actual.to_lowercase().contains(part))
                    && pattern
                        .longer_than
                        .is_none_or(|length| actual.chars().count() > length)
            }
        }
    }
}

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
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) value: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) contains: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) one_of: Option<Vec<Value>>,
}

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) selectable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) editable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) focusable: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Movement {
    pub(crate) target: Target,
    pub(crate) direction: Direction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) since: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) y_within: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) within: Option<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BlankCheck {
    pub(crate) target: Target,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) at: Option<At>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Direction {
    Up,
    Down,
    Left,
    Right,
    None,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub(crate) enum Names {
    Exactly(Vec<String>),
    Including(Box<NameFilter>),
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NameFilter {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) contains: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) excludes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) at_least: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) ends_with: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) starts_with: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) first: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) first_above: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) any_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) toggled: Option<Toggled>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) in_order: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) set: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub(crate) occurrences: std::collections::BTreeMap<String, usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) sorted_casefold_from: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Toggled {
    pub(crate) names: Vec<String>,
    pub(crate) against: Vec<String>,
}

impl Names {
    pub(crate) fn matches(&self, actual: &[String]) -> bool {
        match self {
            Self::Exactly(expected) => expected == actual,
            Self::Including(filter) => {
                filter.contains.iter().all(|name| actual.contains(name))
                    && !filter.excludes.iter().any(|name| actual.contains(name))
                    && filter.count.is_none_or(|count| count == actual.len())
                    && filter.at_least.is_none_or(|least| actual.len() >= least)
                    && actual.ends_with(&filter.ends_with)
                    && actual.starts_with(&filter.starts_with)
                    && filter
                        .first
                        .as_ref()
                        .is_none_or(|first| actual.first() == Some(first))
                    && filter
                        .first_above
                        .as_ref()
                        .is_none_or(|bound| actual.first().is_some_and(|first| first > bound))
                    && filter
                        .any_from
                        .as_ref()
                        .is_none_or(|bound| actual.iter().any(|name| name >= bound))
                    && filter.toggled.as_ref().is_none_or(|toggled| {
                        toggled
                            .names
                            .iter()
                            .all(|name| actual.contains(name) != toggled.against.contains(name))
                    })
                    && in_order(actual, &filter.in_order)
                    && filter.set.as_ref().is_none_or(|set| {
                        actual.iter().all(|name| set.contains(name))
                            && set.iter().all(|name| actual.contains(name))
                    })
                    && filter.occurrences.iter().all(|(name, count)| {
                        actual.iter().filter(|listed| *listed == name).count() == *count
                    })
                    && filter.sorted_casefold_from.is_none_or(|from| {
                        let rest = actual.get(from..).unwrap_or_default();
                        let mut sorted = rest.to_vec();
                        sorted.sort_by_key(|name| name.to_lowercase());
                        sorted == rest
                    })
            }
        }
    }
}

fn in_order(actual: &[String], expected: &[String]) -> bool {
    let positions: Option<Vec<usize>> = expected
        .iter()
        .map(|name| actual.iter().position(|item| item == name))
        .collect();
    positions.is_some_and(|positions| positions.is_sorted())
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
        if let Check::All(predicates) = &check
            && predicates.is_empty()
        {
            return Err(serde::de::Error::custom("all takes at least one predicate"));
        }
        if let Check::Pref(pref) = &check
            && [
                pref.value.is_some(),
                pref.contains.is_some(),
                pref.one_of.is_some(),
            ]
            .into_iter()
            .filter(|given| *given)
            .count()
                != 1
        {
            return Err(serde::de::Error::custom(
                "pref takes exactly one of value, contains, and one_of",
            ));
        }
        if let Check::PanesWith(panes) = &check
            && !matches!(panes.target, Target::Role { within: None, .. })
        {
            return Err(serde::de::Error::custom(
                "panes_with takes a role target without within",
            ));
        }
        if let Check::Mentions(mention) = &check
            && [
                mention.text.is_some(),
                mention.words.is_some(),
                mention.equals.is_some(),
            ]
            .into_iter()
            .filter(|given| *given)
            .count()
                != 1
        {
            return Err(serde::de::Error::custom(
                "mentions takes exactly one of text, words, and equals",
            ));
        }
        if let Check::LargestImage(largest) = &check
            && largest.size.is_some()
            && largest.exceeds.is_some()
        {
            return Err(serde::de::Error::custom(
                "largest_image takes size or exceeds, not both",
            ));
        }
        if let Check::File(file) = &check {
            if !file.path.starts_with('/') {
                return Err(serde::de::Error::custom(
                    "file takes an absolute path, such as $fixture/todo.txt",
                ));
            }
            if file.exists.is_some() == file.mode_bits.is_some() {
                return Err(serde::de::Error::custom(
                    "file takes exactly one of exists and mode_bits",
                ));
            }
            if let Some(bits) = &file.mode_bits
                && u32::from_str_radix(&bits.mask, 8).is_err()
            {
                return Err(serde::de::Error::custom(
                    "mode_bits takes an octal mask such as \"100\"",
                ));
            }
        }
        if field.is_some_and(|field| field.of.is_some() && field.name.is_some()) {
            return Err(serde::de::Error::custom(
                "a field is either remembered (of) or found by name, not both",
            ));
        }
        if dir.is_some() && !check.takes_directory() {
            return Err(serde::de::Error::custom(
                "dir applies only to entries, entry, entry_gone, selected, matches, visible, \
                 scroll_value, and band_selected",
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
    pub(crate) fn parse(name: &str) -> Result<Self, String> {
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

/// A trailing `+` is the plus key itself.
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
