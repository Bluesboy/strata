// SPDX-License-Identifier: MIT

use std::time::Duration;

use serde_json::json;

use super::{
    input,
    protocol::{
        At, Check, Chord, Command, EntryTarget, FieldCheck, Modifier, Offset, Predicate, Presence,
        Query, Target, changed_keys, parse_command,
    },
    state::{self, EntryObservation, FieldState, Observation, PaneObservation},
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
                check: Check::Focused(Presence::Named("todo.txt".to_owned())),
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
    assert_eq!(
        parse_command(r#"{"activate":{"role":"toggle button","name":"Single"},"message":"m"}"#),
        Ok(Command::Activate {
            target: Target::Role {
                role: "toggle button".to_owned(),
                name: "Single".to_owned(),
            },
            message: Some("m".to_owned()),
        })
    );
    assert_eq!(
        parse_command(r#"{"holds":{"dialog":null}}"#),
        Ok(Command::Holds(Predicate {
            check: Check::Dialog(Presence::Absent),
            dir: None,
        }))
    );
    let Ok(Command::Expect {
        predicates,
        message: None,
    }) = parse_command(r#"{"expect":[{"selected":["a"],"dir":"x"},{"focused":null}]}"#)
    else {
        panic!("expect should take a list of predicates");
    };
    assert_eq!(predicates.len(), 2);
    let Ok(Command::Expect { message, .. }) =
        parse_command(r#"{"expect":{"focused":null},"message":"focus to leave the list"}"#)
    else {
        panic!("expect should take the twin's assertion message");
    };
    assert_eq!(message.as_deref(), Some("focus to leave the list"));
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
        r#"{"type":""}"#,
        r#"{"activate":{"entry":"a"}}"#,
        r#"{"remember":{"entry":"a"},"as":"x"}"#,
        r#"{"remember":{"field":true}}"#,
        r#"{"click":{"field":false}}"#,
        r#"{"wait":{"dialog":false}}"#,
        r#"{"wait":{"field":{"text":"a","caret":"end"}}}"#,
        r#"{"wait":{"pref":{"key":"browser_mode"}}}"#,
        r#"{"open_menu":{"header":"Appearance"}}"#,
        r#"{"wait":{"field":{"of":"rename","name":"Rename"}}}"#,
        r#"{"expect":{"editor_within_viewport":{"of":"rename","name":"Rename"}}}"#,
        r#"{"wait":{"any":[{"field":{"of":"a","name":"b"}}]}}"#,
        r#"{"holds":{"focused":"a"},"message":"b"}"#,
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
        ..Observation::default()
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
        ..Observation::default()
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
        dialog: Some("Unable to rename item".to_owned()),
        field: Some(FieldState {
            name: "Rename".to_owned(),
            text: "todo.txt".to_owned(),
            caret: 8,
            selection: Some([0, 4]),
            focused: true,
        }),
        ..Observation::default()
    };
    assert_eq!(
        serde_json::to_value(observation.app_state()).expect("the snapshot should encode"),
        json!({
            "view_mode": "Columns",
            "directory": "root",
            "panes": [{ "name": "root", "selected": ["todo.txt"], "focused": "todo.txt" }],
            "focused": "todo.txt",
            "focused_pane": "root",
            "dialog": "Unable to rename item",
            "menu": null,
            "preview": null,
            "field": {
                "name": "Rename", "text": "todo.txt", "caret": 8,
                "selection": [0, 4], "focused": true,
            },
            "matches": ["archive", "todo.txt"],
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
    assert_eq!(target(r#"{"field":true}"#), Target::Field(None));
    assert_eq!(
        target(r#"{"field":"filter"}"#),
        Target::Field(Some("filter".to_owned()))
    );
    assert_eq!(
        target(r#"{"result":"beta/match-note.txt"}"#),
        Target::Result("beta/match-note.txt".to_owned())
    );
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
        r#"{"field":"filter"}"#,
        r#"{"result":"beta/match-note.txt"}"#,
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
        focused_pane: Some(0),
        window_width: Some(420),
        ..Observation::default()
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

#[test]
fn dialog_menu_preview_and_match_predicates_read_the_snapshot() {
    let observation = Observation {
        panes: vec![pane("root", vec![entry("readme.md", false)])],
        focused_pane: Some(0),
        dialog: Some("Unable to rename item".to_owned()),
        menu: Some(vec!["New File".to_owned(), "New Folder".to_owned()]),
        preview: Some(vec!["Preview".to_owned(), "beta source\n".to_owned()]),
        results: Some(vec![
            "match-note.txt".to_owned(),
            "match-note.txt".to_owned(),
        ]),
        ..Observation::default()
    };
    for holding in [
        json!({ "dialog": "Unable to rename item" }),
        json!({ "dialog": true }),
        json!({ "focused_pane": "root" }),
        json!({ "focused": null }),
        json!({ "menu_items": {} }),
        json!({ "menu_items": { "contains": ["New Folder"], "excludes": ["Quick preview"] } }),
        json!({ "preview": "beta source" }),
        json!({ "matches": { "count": 2 } }),
        json!({ "matches": ["match-note.txt", "match-note.txt"] }),
        json!({ "any": [{ "focused": true }, { "focused_pane": true }] }),
    ] {
        assert!(
            observation.evaluate(&predicate(holding.clone())).is_ok(),
            "{holding} should hold"
        );
    }
    for failing in [
        json!({ "dialog": null }),
        json!({ "menu_items": null }),
        json!({ "preview": null }),
        json!({ "preview": "alpha source" }),
        json!({ "matches": { "count": 3 } }),
        json!({ "any": [{ "focused": true }, { "dialog": null }] }),
    ] {
        assert!(
            observation.evaluate(&predicate(failing.clone())).is_err(),
            "{failing} should not hold"
        );
    }
    let closed = Observation {
        panes: vec![pane("root", vec![entry("readme.md", false)])],
        ..Observation::default()
    };
    for holding in [
        json!({ "dialog": null }),
        json!({ "menu_items": null }),
        json!({ "preview": null }),
        json!({ "matches": ["readme.md"] }),
    ] {
        assert!(
            closed.evaluate(&predicate(holding.clone())).is_ok(),
            "{holding} should hold with nothing open"
        );
    }
    assert!(
        closed
            .evaluate(&predicate(json!({ "menu_items": {} })))
            .is_err()
    );
}

#[test]
fn a_field_check_matches_only_the_properties_it_names() {
    let field = FieldState {
        name: "Rename".to_owned(),
        text: "report.txt".to_owned(),
        caret: 6,
        selection: None,
        focused: true,
    };
    let check = |json: serde_json::Value| -> FieldCheck {
        serde_json::from_value(json).expect("the field check should parse")
    };
    for holding in [
        json!({}),
        json!({ "name": "Rename", "text": "report.txt", "focused": true }),
        json!({ "text": { "ends_with": ".txt", "starts_with": "rep" } }),
        json!({ "caret": 6, "selection": null }),
        json!({ "caret": { "min": 1, "max": 9 } }),
    ] {
        assert!(
            state::field_matches(&check(holding.clone()), Some(&field)),
            "{holding} should match"
        );
    }
    for failing in [
        json!({ "text": "report" }),
        json!({ "caret": { "min": 7, "max": 9 } }),
        json!({ "selection": [0, 6] }),
        json!({ "focused": false }),
    ] {
        assert!(
            !state::field_matches(&check(failing.clone()), Some(&field)),
            "{failing} should not match"
        );
    }
    assert!(!state::field_matches(&check(json!({})), None));
}

#[test]
fn an_editor_is_within_the_viewport_only_inside_the_pane_and_window() {
    assert!(state::within_viewport([210, 40, 200, 24], 200, 420));
    assert!(state::within_viewport([200, 40, 220, 24], 200, 420));
    assert!(!state::within_viewport([190, 40, 100, 24], 200, 420));
    assert!(!state::within_viewport([210, 40, 221, 24], 200, 420));
    assert!(!state::within_viewport([210, 40, 100, 0], 200, 420));
    assert!(!state::within_viewport([420, 40, 0, 24], 200, 420));
}

#[test]
fn a_native_toggle_reports_its_own_state_over_a_recorded_one() {
    crate::test_support::gtk_test(
        "qa::tests::a_native_toggle_reports_its_own_state_over_a_recorded_one",
        || {
            use gtk::prelude::*;
            let toggle = gtk::ToggleButton::new();
            toggle.set_active(true);
            crate::ui::accessibility_qa::record(&toggle, |naming| naming.pressed = Some(false));
            assert!(super::targets::states(toggle.upcast_ref()).pressed);

            let plain = gtk::Button::new();
            crate::ui::accessibility_qa::record(&plain, |naming| {
                naming.pressed = Some(true);
                naming.checked = Some(true);
            });
            let states = super::targets::states(plain.upcast_ref());
            assert!(states.pressed && states.checked);

            let switch = gtk::Switch::new();
            switch.set_active(true);
            assert!(super::targets::states(switch.upcast_ref()).checked);
        },
    );
}
