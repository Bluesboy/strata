// SPDX-License-Identifier: MIT

//! One JSON command per line in, one [`Reply`] per line out.
//!
//! A command is an object with exactly one command key (`key`, `click`,
//! `wait`, `expect`, `query`) whose value is its main argument, plus that
//! command's optional fields. Add a command by adding its struct and a
//! [`COMMANDS`] arm; add a predicate with a [`Check`] variant.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub(crate) const DEFAULT_WAIT: Duration = Duration::from_millis(10_000);

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Command {
    Key {
        chord: Chord,
        repeat: u32,
    },
    Click {
        target: Target,
        button: u32,
        count: u32,
        modifiers: Vec<Modifier>,
    },
    /// Click an entry until it reports itself selected, as `select_entry` does.
    Select(EntryTarget),
    Wait {
        predicate: Predicate,
        timeout: Duration,
        message: Option<String>,
    },
    Expect {
        predicates: Vec<Predicate>,
    },
    Query(Query),
}

const COMMANDS: [&str; 6] = ["key", "click", "select", "wait", "expect", "query"];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyFields {
    key: String,
    #[serde(default = "one")]
    repeat: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClickFields {
    click: Target,
    #[serde(default = "one")]
    button: u32,
    #[serde(default = "one")]
    count: u32,
    #[serde(default)]
    mods: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectFields {
    select: EntryTarget,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WaitFields {
    wait: Predicate,
    timeout_ms: Option<u64>,
    message: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum OneOrMany {
    One(Predicate),
    Many(Vec<Predicate>),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectFields {
    expect: OneOrMany,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryFields {
    query: Query,
}

fn one() -> u32 {
    1
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Query {
    State,
    Tree,
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
        "click" => {
            let fields: ClickFields = serde_json::from_value(fields).map_err(invalid)?;
            if !matches!(fields.button, 1..=3) || !matches!(fields.count, 1 | 2) {
                return Err("click takes button 1-3 and count 1 or 2".to_owned());
            }
            Ok(Command::Click {
                target: fields.click,
                button: fields.button,
                count: fields.count,
                modifiers: fields
                    .mods
                    .iter()
                    .map(|name| Modifier::parse(name))
                    .collect::<Result<_, _>>()?,
            })
        }
        "select" => {
            let fields: SelectFields = serde_json::from_value(fields).map_err(invalid)?;
            Ok(Command::Select(fields.select))
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
                OneOrMany::One(predicate) => vec![predicate],
                OneOrMany::Many(predicates) if !predicates.is_empty() => predicates,
                OneOrMany::Many(_) => return Err("expect needs a predicate".to_owned()),
            };
            Ok(Command::Expect { predicates })
        }
        _ => {
            let fields: QueryFields = serde_json::from_value(fields).map_err(invalid)?;
            Ok(Command::Query(fields.query))
        }
    }
}

/// Names a widget the way a person sees it, never by screen position.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub(crate) enum Target {
    Entry(EntryTarget),
}

/// An entry row by name, in the named pane or in any pane on screen.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EntryTarget {
    pub(crate) entry: String,
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
    Entries(Names),
    Selected(Names),
    Focused(Option<String>),
    ViewMode(String),
}

impl Check {
    fn takes_directory(&self) -> bool {
        matches!(self, Self::Entries(_) | Self::Selected(_))
    }
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
}

impl Names {
    pub(crate) fn matches(&self, actual: &[String]) -> bool {
        match self {
            Self::Exactly(expected) => expected == actual,
            Self::Including(filter) => {
                filter.contains.iter().all(|name| actual.contains(name))
                    && !filter.excludes.iter().any(|name| actual.contains(name))
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
        if dir.is_some() && !check.takes_directory() {
            return Err(serde::de::Error::custom(
                "dir applies only to entries and selected",
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
