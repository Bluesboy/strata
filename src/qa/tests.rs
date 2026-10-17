// SPDX-License-Identifier: MIT

use std::time::Duration;

use serde_json::json;

use super::{
    input,
    protocol::{
        Check, Chord, Command, EntryTarget, Modifier, Predicate, Query, Target, changed_keys,
        parse_command,
    },
    state::{EntryObservation, Observation, PaneObservation},
};

fn entry(name: &str, selected: bool) -> EntryObservation {
    EntryObservation {
        name: name.to_owned(),
        selected,
        focused: false,
    }
}

fn pane(name: &str, entries: Vec<EntryObservation>) -> PaneObservation {
    PaneObservation {
        name: name.to_owned(),
        view: "Columns",
        entries,
    }
}

fn predicate(json: serde_json::Value) -> Predicate {
    serde_json::from_value(json).expect("the predicate should parse")
}

#[test]
fn commands_parse_from_the_scenario_spelling_with_their_defaults() {
    assert_eq!(
        parse_command(r#"{"key":"ctrl+shift+n"}"#),
        Ok(Command::Key {
            chord: Chord {
                modifiers: vec![Modifier::Ctrl, Modifier::Shift],
                key: "n".to_owned(),
            },
            repeat: 1,
        })
    );
    assert_eq!(
        parse_command(r#"{"wait":{"focused":"todo.txt"}}"#),
        Ok(Command::Wait {
            predicate: Predicate {
                check: Check::Focused(Some("todo.txt".to_owned())),
                dir: None,
            },
            timeout: Duration::from_millis(10_000),
            message: None,
        })
    );
    assert_eq!(
        parse_command(r#"{"click":{"entry":"readme.md","dir":"docs"}}"#),
        Ok(Command::Click {
            target: Target::Entry(EntryTarget {
                entry: "readme.md".to_owned(),
                dir: Some("docs".to_owned()),
            }),
            button: 1,
            count: 1,
            modifiers: Vec::new(),
        })
    );
    assert_eq!(
        parse_command(r#"{"query":"tree"}"#),
        Ok(Command::Query(Query::Tree))
    );
    assert_eq!(
        parse_command(r#"{"select":{"entry":"readme.md"}}"#),
        Ok(Command::Select(EntryTarget {
            entry: "readme.md".to_owned(),
            dir: None,
        }))
    );
    let Ok(Command::Expect { predicates }) =
        parse_command(r#"{"expect":[{"selected":["a"],"dir":"x"},{"focused":null}]}"#)
    else {
        panic!("expect should take a list of predicates");
    };
    assert_eq!(predicates.len(), 2);
}

#[test]
fn malformed_commands_are_rejected_instead_of_guessed() {
    for line in [
        r#"{"key":"Down","wait":{"focused":"a"}}"#,
        r#"{"key":"Down","repeats":2}"#,
        r#"{"wait":{"focused":"a","directory":"b"}}"#,
        r#"{"wait":{"directory":"b","dir":"c"}}"#,
        r#"{"click":{"entry":"a","name":"b"}}"#,
        r#"{"click":{"entry":"a"},"button":4}"#,
        r#"{"key":"hyper+a"}"#,
        r#"{"expect":[]}"#,
        r#"{"type":"abc"}"#,
        "[]",
    ] {
        assert!(parse_command(line).is_err(), "{line} should be rejected");
    }
}

#[test]
fn a_trailing_plus_is_the_plus_key() {
    assert_eq!(
        Chord::parse("ctrl++"),
        Ok(Chord {
            modifiers: vec![Modifier::Ctrl],
            key: "+".to_owned(),
        })
    );
    assert_eq!(
        Chord::parse("+"),
        Ok(Chord {
            modifiers: Vec::new(),
            key: "+".to_owned(),
        })
    );
}

#[test]
fn single_characters_are_latin1_or_unicode_keysyms() {
    assert_eq!(input::keysym("j"), Ok(0x6a));
    assert_eq!(input::keysym("é"), Ok(0xe9));
    assert_eq!(input::keysym("о"), Ok(0x0100_043e));
    assert!(input::keysym("NotAKey").is_err());
}

#[test]
fn the_current_directory_is_the_focused_pane_or_else_the_deepest() {
    let mut observation = Observation {
        panes: vec![pane("root", vec![]), pane("documents", vec![])],
        focused_entry: None,
        focused_pane: None,
    };
    assert_eq!(observation.directory(), Some("documents"));
    observation.focused_pane = Some(0);
    assert_eq!(observation.directory(), Some("root"));
    assert!(
        observation
            .evaluate(&predicate(json!({ "directory": "root" })))
            .is_ok()
    );
}

#[test]
fn entry_predicates_read_the_named_pane_or_the_deepest() {
    let observation = Observation {
        panes: vec![
            pane(
                "root",
                vec![entry("archive", true), entry("todo.txt", false)],
            ),
            pane("archive", vec![entry("old.txt", false)]),
        ],
        focused_entry: Some("archive".to_owned()),
        focused_pane: Some(0),
    };
    for holding in [
        json!({ "entries": ["old.txt"] }),
        json!({ "entries": ["archive", "todo.txt"], "dir": "root" }),
        json!({ "entries": { "contains": ["todo.txt"] }, "dir": "root" }),
        json!({ "entries": { "excludes": ["todo.txt"] } }),
        json!({ "selected": ["archive"], "dir": "root" }),
        json!({ "selected": [] }),
        json!({ "panes": ["root", "archive"] }),
        json!({ "focused": "archive" }),
        json!({ "view_mode": "Columns" }),
    ] {
        assert!(
            observation.evaluate(&predicate(holding.clone())).is_ok(),
            "{holding} should hold"
        );
    }
    assert!(observation.entry_selected("archive", None));
    assert!(!observation.entry_selected("archive", Some("archive")));
    assert!(!observation.entry_selected("todo.txt", None));
    for failing in [
        json!({ "entries": ["todo.txt", "archive"], "dir": "root" }),
        json!({ "entries": { "contains": ["todo.txt"] } }),
        json!({ "focused": null }),
        json!({ "directory": "archive" }),
    ] {
        assert!(
            observation.evaluate(&predicate(failing.clone())).is_err(),
            "{failing} should not hold"
        );
    }
    let failure = observation
        .evaluate(&predicate(
            json!({ "selected": ["todo.txt"], "dir": "root" }),
        ))
        .expect_err("the selection differs");
    assert_eq!(failure.actual, json!(["archive"]));
    assert_eq!(
        failure.predicate,
        json!({ "selected": ["todo.txt"], "dir": "root" })
    );
    assert_eq!(
        observation
            .evaluate(&predicate(json!({ "entries": ["x"], "dir": "missing" })))
            .expect_err("no such pane")
            .actual,
        json!([])
    );
}

#[test]
fn the_snapshot_reports_selection_and_focus_per_pane() {
    let mut focused = entry("todo.txt", true);
    focused.focused = true;
    let observation = Observation {
        panes: vec![pane("root", vec![entry("archive", false), focused])],
        focused_entry: Some("todo.txt".to_owned()),
        focused_pane: Some(0),
    };
    assert_eq!(
        serde_json::to_value(observation.app_state()).expect("the snapshot should encode"),
        json!({
            "view_mode": "Columns",
            "directory": "root",
            "panes": [{ "name": "root", "selected": ["todo.txt"], "focused": "todo.txt" }],
            "focused": "todo.txt",
        })
    );
}

#[test]
fn changed_lists_only_the_keys_that_differ() {
    let before = json!({ "directory": "a", "focused": "x", "panes": [] });
    let after = json!({ "directory": "a", "focused": "y", "panes": [] });
    assert_eq!(changed_keys(Some(&before), &after), ["focused"]);
    assert_eq!(changed_keys(None, &before).len(), 3);
}
