// SPDX-License-Identifier: MIT

use std::time::Duration;

use serde_json::json;

use super::{
    input,
    protocol::{
        At, Check, Chord, Command, EntryTarget, Modifier, Offset, Predicate, Query, Target,
        changed_keys, parse_command,
    },
    state::{EntryObservation, Observation, PaneObservation},
    step_towards,
    targets::Rect,
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
            timeout: Duration::from_millis(15_000),
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
            at: None,
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
    assert_eq!(
        parse_command(r#"{"wheel":{"viewport":null},"at":["50%",-14],"direction":"up"}"#),
        Ok(Command::Wheel {
            target: Target::Viewport { dir: None },
            at: Some(At {
                x: Offset::Percent(50),
                y: Offset::Pixels(-14),
            }),
            clicks: 3,
            down: false,
            modifiers: Vec::new(),
        })
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
        r#"{"click":{"entry":"a"},"button":2}"#,
        r#"{"click":{"background":true},"at":[1,2]}"#,
        r#"{"click":{"background":false}}"#,
        r#"{"click":{"sidebar":"Home","header":"Size"}}"#,
        r#"{"click":{"entry":"a"},"at":[1]}"#,
        r#"{"click":{"entry":"a"},"at":["101%",0]}"#,
        r#"{"move":{"pane":"a","dir":"b"}}"#,
        r#"{"wheel":{"viewport":null},"clicks":0}"#,
        r#"{"wheel":{"viewport":null},"direction":"sideways"}"#,
        r#"{"drag":{"from":{"entry":"a"}}}"#,
        r#"{"drag":{"from":{"entry":"a"},"to":{"entry":"b"}},"steps":0}"#,
        r#"{"resize":{"width":0,"height":300}}"#,
        r#"{"wait":{"present":{"entry":"a"}},"dir":"x"}"#,
        r#"{"wait":{"moved":{"target":{"entry":"a"},"direction":"diagonal"}}}"#,
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
        window_width: None,
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
        window_width: Some(1200),
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
        window_width: None,
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

#[test]
fn every_target_kind_parses_from_its_key() {
    let target = |json: &str| -> Target {
        let Ok(Command::Move { target, .. }) = parse_command(&format!(r#"{{"move":{json}}}"#))
        else {
            panic!("{json} should be a target");
        };
        target
    };
    assert_eq!(
        target(r#"{"entry":"todo.txt","dir":"archive"}"#),
        Target::Entry(EntryTarget {
            entry: "todo.txt".to_owned(),
            dir: Some("archive".to_owned()),
        })
    );
    assert_eq!(
        target(r#"{"role":"button","name":"Keep Both"}"#),
        Target::Role {
            role: "button".to_owned(),
            name: "Keep Both".to_owned(),
        }
    );
    assert_eq!(
        target(r#"{"background":true,"dir":"archive"}"#),
        Target::Background {
            dir: Some("archive".to_owned())
        }
    );
    assert_eq!(target(r#"{"pane":null}"#), Target::Pane { dir: None });
    assert_eq!(
        target(r#"{"viewport":"archive"}"#),
        Target::Viewport {
            dir: Some("archive".to_owned())
        }
    );
    assert_eq!(target(r#"{"field":true}"#), Target::Field);
    assert_eq!(
        target(r#"{"sidebar":"Home"}"#),
        Target::Sidebar("Home".to_owned())
    );
    assert_eq!(
        target(r#"{"header":"Size"}"#),
        Target::Header("Size".to_owned())
    );
    assert_eq!(
        target(r#"{"menu_item":"New File"}"#),
        Target::MenuItem("New File".to_owned())
    );
    assert_eq!(
        target(r#"{"dialog_button":"Close"}"#),
        Target::DialogButton("Close".to_owned())
    );
    for json in [
        r#"{"entry":"todo.txt","dir":"archive"}"#,
        r#"{"background":true}"#,
        r#"{"pane":null}"#,
        r#"{"menu_item":"New File"}"#,
    ] {
        assert_eq!(
            target(json).to_json(),
            serde_json::from_str::<serde_json::Value>(json).expect("the JSON should parse"),
            "{json} should round-trip, since settle keys targets by their JSON"
        );
    }
}

#[test]
fn a_bad_target_names_the_kind_and_the_problem() {
    let error = |line: &str| parse_command(line).expect_err("the target should be rejected");
    assert!(
        error(r#"{"click":{"entry":"a","name":"b"}}"#).contains("invalid entry target"),
        "an unknown field names the target kind"
    );
    assert!(error(r#"{"click":{"name":"b"}}"#).contains("a target needs exactly one of"));
    assert!(error(r#"{"click":"todo.txt"}"#).contains("a target is an object"));
}

#[test]
fn at_offsets_resolve_like_the_python_points() {
    let bounds = Rect {
        x: 100.0,
        y: 40.0,
        width: 301.0,
        height: 95.0,
    };
    // `bounds.center`, then `background_point`-style offsets.
    assert_eq!(bounds.point(None), (250.0, 87.0));
    let at = |x: Offset, y: Offset| At { x, y };
    assert_eq!(
        bounds.point(Some(&at(Offset::Percent(50), Offset::Percent(80)))),
        (250.0, 116.0)
    );
    assert_eq!(
        bounds.point(Some(&at(Offset::Pixels(4), Offset::Pixels(-14)))),
        (104.0, 121.0)
    );
}

#[test]
fn drag_steps_floor_towards_the_target_as_python_does() {
    let path: Vec<i16> = (1..=4).map(|step| step_towards(100, 10, step, 4)).collect();
    assert_eq!(path, [77, 55, 32, 10]);
    let path: Vec<i16> = (1..=3).map(|step| step_towards(10, 20, step, 3)).collect();
    assert_eq!(path, [13, 16, 20]);
}

#[test]
fn entry_pane_and_width_predicates_read_the_snapshot() {
    let observation = Observation {
        panes: vec![
            pane(
                "root",
                vec![entry("archive", true), entry("todo.txt", false)],
            ),
            pane("archive", vec![entry("old.txt", false)]),
        ],
        focused_entry: None,
        focused_pane: Some(0),
        window_width: Some(420),
    };
    for holding in [
        json!({ "pane": "archive" }),
        json!({ "entry": "todo.txt" }),
        json!({ "entry": "old.txt", "dir": "archive" }),
        json!({ "entry_gone": "todo.txt", "dir": "archive" }),
        json!({ "entry_gone": "missing.txt" }),
        json!({ "window_width": 420 }),
    ] {
        assert!(
            observation.evaluate(&predicate(holding.clone())).is_ok(),
            "{holding} should hold"
        );
    }
    for failing in [
        json!({ "pane": "root" }),
        json!({ "entry": "todo.txt", "dir": "archive" }),
        json!({ "entry_gone": "todo.txt" }),
        json!({ "window_width": 640 }),
    ] {
        assert!(
            observation.evaluate(&predicate(failing.clone())).is_err(),
            "{failing} should not hold"
        );
    }
}
