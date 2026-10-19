// SPDX-License-Identifier: MIT

use std::time::Duration;

use serde_json::json;

use super::{
    Shape, input,
    pixels::{self, Capture},
    protocol::{
        Accessor, At, Axis, Change, Check, Chord, Command, EntryPart, EntryTarget, FieldCheck,
        Modifier, Names, Offset, Predicate, Presence, Query, StateName, Target, TextMatch,
        changed_keys, parse_command,
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
            samples: 1,
            settle: None,
            never: None,
        })
    );
    assert_eq!(
        parse_command(r#"{"click":{"entry":"readme.md","dir":"docs"}}"#),
        Ok(Command::Click {
            target: Target::Entry(EntryTarget {
                entry: "readme.md".to_owned(),
                dir: Some("docs".to_owned()),
                part: None,
            }),
            button: 1,
            count: 1,
            modifiers: Vec::new(),
            at: None,
            left_bound: None,
            release_modifiers_first: false,
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
            part: None,
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
            until: None,
        })
    );
    assert_eq!(
        parse_command(r#"{"activate":{"role":"toggle button","name":"Single"},"message":"m"}"#),
        Ok(Command::Activate {
            target: Target::Role {
                role: "toggle button".to_owned(),
                other_roles: Vec::new(),
                name: Some("Single".to_owned()),
                description: None,
                within: None,
                name_ends_with: Vec::new(),
                nth: None,
                states: Vec::new(),
                name_matches: None,
                has: None,
                rendered: true,
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
        timeout: None,
        hold: None,
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
        r#"{"remember":{"field":"kept"},"as":"x"}"#,
        r#"{"note":"entries"}"#,
        r#"{"note":"directory","dir":"x","as":"a"}"#,
        r#"{"note":"focused","from":"a","as":"b"}"#,
        r#"{"note":"middle_visible","as":"a"}"#,
        r#"{"note":"entries","unless_first":"a","as":"b"}"#,
        r#"{"note":"entries","message":"m","as":"a"}"#,
        r#"{"click":{"entry":"a"},"release_mods_first":true}"#,
        r#"{"click":{"entry":"a"},"count":2,"mods":["shift"],"release_mods_first":true}"#,
        r#"{"click":{"window":false}}"#,
        r#"{"click":{"entry":"a","part":"name"}}"#,
        r#"{"button_down":4}"#,
        r#"{"set_scrollbar":"diagonal","value":0}"#,
        r#"{"wait":{"focused":"a"},"samples":0}"#,
        r#"{"wait":{"focused":{"one_of":"a"}}}"#,
        r#"{"wait":{"not":{"selected":[]},"dir":"x"}}"#,
        r#"{"drag":{"from":{"entry":"a"},"to":{"entry":"b"}},"message":"m"}"#,
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
fn a_note_waits_only_when_asked_and_a_drag_checks_after_its_press() {
    let Ok(Command::Note {
        value,
        dir,
        wait,
        from,
        through,
        ..
    }) = parse_command(
        r#"{"note":"entries","dir":"root","from":"documents","through":"todo.txt","as":"range"}"#,
    )
    else {
        panic!("a sliced note should parse");
    };
    assert_eq!(value, Accessor::Entries);
    assert_eq!(dir.as_deref(), Some("root"));
    assert_eq!(wait, None);
    assert_eq!(
        (from.as_deref(), through.as_deref()),
        (Some("documents"), Some("todo.txt"))
    );
    let Ok(Command::Note { wait, .. }) = parse_command(
        r#"{"note":"middle_visible","unless_first":"folder-000","until_found":true,"as":"first"}"#,
    ) else {
        panic!("a waiting note should parse");
    };
    assert_eq!(wait, Some(Duration::from_millis(15_000)));
    let Ok(Command::Drag {
        from_at,
        after_press,
        ..
    }) = parse_command(
        r#"{"drag":{"from":{"pane":"a"},"from_at":["50%",-60],"to":{"entry":"b"}},"after_press":{"selected":[],"dir":"a"},"message":"m"}"#,
    )
    else {
        panic!("a drag with a check after its press should parse");
    };
    assert_eq!(
        from_at,
        Some(At {
            x: Offset::Percent(50),
            y: Offset::Pixels(-60)
        })
    );
    let [check] = after_press.as_slice() else {
        panic!("one predicate is one check");
    };
    assert_eq!(check.predicate.dir.as_deref(), Some("a"));
    assert_eq!((check.wait, check.message.as_deref()), (None, Some("m")));
    let Ok(Command::Drag { after_press, .. }) = parse_command(
        r#"{"drag":{"from":{"entry":"a"},"to":{"entry":"b"}},"after_press":[{"wait":{"entry":"a"},"message":"w"},{"expect":{"preview":null},"message":"e"}]}"#,
    ) else {
        panic!("a drag with a list of checks after its press should parse");
    };
    let waits: Vec<(Option<Duration>, Option<&str>)> = after_press
        .iter()
        .map(|check| (check.wait, check.message.as_deref()))
        .collect();
    assert_eq!(
        waits,
        [
            (Some(Duration::from_millis(15_000)), Some("w")),
            (None, Some("e"))
        ]
    );
    for bad in [
        r#"{"drag":{"from":{"entry":"a"},"to":{"entry":"b"}},"after_press":[{"wait":{"entry":"a"}}],"message":"m"}"#,
        r#"{"drag":{"from":{"entry":"a"},"to":{"entry":"b"}},"after_press":[{"expect":{"entry":"a"},"timeout_ms":5}]}"#,
        r#"{"drag":{"from":{"entry":"a"},"to":{"entry":"b"}},"after_press":[]}"#,
    ] {
        assert!(parse_command(bad).is_err(), "{bad} should be rejected");
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
            showing: true,
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
            part: None,
        })
    );
    assert_eq!(
        target(r#"{"role":"button","name":"Keep Both"}"#),
        Target::Role {
            role: "button".to_owned(),
            other_roles: Vec::new(),
            name: Some("Keep Both".to_owned()),
            description: None,
            within: None,
            name_ends_with: Vec::new(),
            nth: None,
            states: Vec::new(),
            name_matches: None,
            has: None,
            rendered: true,
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
        target(r#"{"result_row":-1,"containing":"match-note"}"#),
        Target::ResultRow {
            index: -1,
            containing: Some("match-note".to_owned()),
            name: None,
        }
    );
    assert_eq!(
        target(r#"{"role":"list item","name_ends_with":["/a"],"nth":-1}"#),
        Target::Role {
            role: "list item".to_owned(),
            other_roles: Vec::new(),
            name: None,
            description: None,
            within: None,
            name_ends_with: vec!["/a".to_owned()],
            nth: Some(-1),
            states: Vec::new(),
            name_matches: None,
            has: None,
            rendered: true,
        }
    );
    assert_eq!(target(r#"{"focused_widget":true}"#), Target::FocusedWidget);
    assert_eq!(
        target(r#"{"remembered":"focused"}"#),
        Target::Remembered("focused".to_owned())
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
        r#"{"role":"button","name":"Next video","within":{"preview":true}}"#,
        r#"{"field":"filter"}"#,
        r#"{"result":"beta/match-note.txt"}"#,
        r#"{"result_row":0,"name":"match-note.txt"}"#,
        r#"{"role":"button","name":"drafts","nth":-1}"#,
        r#"{"role":"list item","name_ends_with":["/a","/b"]}"#,
        r#"{"focused_widget":true}"#,
        r#"{"remembered":"focused"}"#,
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
    assert!(
        error(r#"{"click":{"result_row":0,"containing":"a","name":"b"}}"#)
            .contains("containing or name, not both")
    );
    assert!(error(r#"{"click":{"focused_widget":false}}"#).contains("must be true"));
}

#[test]
fn at_offsets_resolve_like_the_python_points() {
    let bounds = Rect {
        x: 100.0,
        y: 40.0,
        width: 301.0,
        height: 95.0,
    };
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
    assert_eq!(
        bounds.point(Some(&at(Offset::Fraction(2, 3), Offset::FromStart(-6)))),
        (300.0, 34.0)
    );
    let Ok(Command::Move {
        at: Some(parsed), ..
    }) = parse_command(r#"{"move":{"pane":null},"at":["2/3","start-6"]}"#)
    else {
        panic!("fractions and near-edge offsets should parse");
    };
    assert_eq!(parsed, at(Offset::Fraction(2, 3), Offset::FromStart(-6)));
    assert_eq!(
        bounds.point(Some(&at(
            Offset::FractionPlus(2, 3, 3),
            Offset::EndLessFraction(1, 6)
        ))),
        (303.0, 120.0)
    );
    assert_eq!(
        bounds.point(Some(&at(Offset::FromEnd(60), Offset::FromEnd(-8)))),
        (461.0, 127.0)
    );
    let Ok(Command::Move {
        at: Some(parsed), ..
    }) = parse_command(r#"{"move":{"pane":null},"at":["2/3+3","end-1/6"]}"#)
    else {
        panic!("shares with pixels and shares from the far edge should parse");
    };
    assert_eq!(
        parsed,
        at(Offset::FractionPlus(2, 3, 3), Offset::EndLessFraction(1, 6))
    );
    let Ok(Command::Move {
        at: Some(parsed), ..
    }) = parse_command(r#"{"move":{"pane":null},"at":["end+60","end-2"]}"#)
    else {
        panic!("far-edge offsets should parse");
    };
    assert_eq!(parsed, at(Offset::FromEnd(60), Offset::FromEnd(-2)));
    for bad in [
        r#"["3/2",0]"#,
        r#"["end2",0]"#,
        r#"["start6",0]"#,
        r#"["2/3+x",0]"#,
        r#"["end+1/6",0]"#,
    ] {
        assert!(
            parse_command(&format!(r#"{{"move":{{"pane":null}},"at":{bad}}}"#)).is_err(),
            "{bad} should be rejected"
        );
    }
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
        json!({ "preview": true }),
        json!({ "peek": false }),
        json!({ "matches": { "count": 2 } }),
        json!({ "matches": { "at_least": 2, "ends_with": ["match-note.txt"] } }),
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
        json!({ "peek": true }),
        json!({ "matches": { "count": 3 } }),
        json!({ "matches": { "at_least": 3 } }),
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
            .evaluate(&predicate(json!({ "preview": true })))
            .is_err()
    );
    assert!(
        closed
            .evaluate(&predicate(json!({ "menu_items": {} })))
            .is_err()
    );
}

#[test]
fn selection_focus_and_negated_predicates_read_every_pane() {
    let mut cursor = entry("notes.txt", false);
    cursor.focused = true;
    let observation = Observation {
        panes: vec![
            pane(
                "root",
                vec![entry("documents", true), entry("todo.txt", true)],
            ),
            pane("documents", vec![cursor, entry("report.md", true)]),
        ],
        focused_entry: Some("notes.txt".to_owned()),
        focused_pane: Some(1),
        results: Some(vec!["notes.txt".to_owned()]),
        ..Observation::default()
    };
    for holding in [
        json!({ "all_selected": ["documents", "todo.txt", "report.md"] }),
        json!({ "panes": { "ends_with": ["root", "documents"], "excludes": ["archive"] } }),
        json!({ "focused": { "one_of": ["readme.md", "notes.txt"] } }),
        json!({ "focused_in_matches": true }),
        json!({ "not": { "selected": [], "dir": "root" } }),
        json!({ "not": { "directory": "root" } }),
    ] {
        assert!(
            observation.evaluate(&predicate(holding.clone())).is_ok(),
            "{holding} should hold"
        );
    }
    for failing in [
        json!({ "all_selected": [] }),
        json!({ "panes": { "ends_with": ["documents", "root"] } }),
        json!({ "focused": { "one_of": ["readme.md"] } }),
        json!({ "focused_in_matches": false }),
        json!({ "not": { "selected": ["report.md"] } }),
    ] {
        assert!(
            observation.evaluate(&predicate(failing.clone())).is_err(),
            "{failing} should not hold"
        );
    }
}

#[test]
fn window_geometry_checks_mirror_the_twin_arithmetic() {
    let window = [0, 0, 1200, 760];
    assert!(state::on_screen([10, 700, 100, 60], window));
    assert!(!state::on_screen([10, 701, 100, 60], window));
    assert!(!state::on_screen([10, 10, 100, 0], window));
    assert!(state::contains([10, 20, 30, 40], (10, 59)));
    assert!(!state::contains([10, 20, 30, 40], (40, 30)));

    let rows: Vec<(String, [i64; 4])> = (0..8)
        .map(|index| (format!("folder-{index:03}"), [0, index * 30, 100, 30]))
        .collect();
    assert_eq!(
        state::middle_visible(&rows, [0, 40, 100, 120], "folder-000"),
        Some("folder-003".to_owned())
    );
    assert_eq!(
        state::middle_visible(&rows, [0, -10, 100, 120], "folder-000"),
        None,
        "nothing scrolled yet while the first visible row is the first entry"
    );
    assert_eq!(
        state::middle_visible(&rows, [0, 400, 100, 60], "folder-000"),
        None
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
        showing: true,
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
fn a_created_editor_starts_beside_the_pane_and_ends_inside_the_window() {
    assert!(state::within_window([210, 40, 200, 24], 200, 420));
    assert!(state::within_window([0, 40, 2, 24], -30, 420));
    assert!(!state::within_window([-1, 40, 100, 24], -30, 420));
    assert!(!state::within_window([190, 40, 100, 24], 200, 420));
    assert!(!state::within_window([210, 40, 211, 24], 200, 420));
    assert!(!state::within_window([210, 40, 1, 24], 200, 420));
}

#[test]
fn a_box_is_inside_another_on_the_axes_asked_for() {
    use super::protocol::PointAxis;
    let viewport = [200, 100, 400, 300];
    assert!(state::inside([210, 110, 100, 20], viewport, None));
    assert!(!state::inside([210, 390, 100, 20], viewport, None));
    assert!(!state::inside([150, 110, 100, 20], viewport, None));
    assert!(state::inside(
        [150, 110, 100, 20],
        viewport,
        Some(PointAxis::Y)
    ));
    assert!(!state::inside(
        [210, 110, 0, 20],
        viewport,
        Some(PointAxis::Y)
    ));
    assert_eq!(
        state::collapse_whitespace(" doesn't\n support  Trash "),
        "doesn't support Trash"
    );
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

#[test]
fn composed_and_kept_targets_parse_and_round_trip() {
    let target = |json: &str| -> Target {
        let Ok(Command::Move { target, .. }) = parse_command(&format!(r#"{{"move":{json}}}"#))
        else {
            panic!("{json} should be a target");
        };
        target
    };
    assert_eq!(
        target(
            r#"{"point":[{"of":{"pane":null},"at":2},{"mid":[{"of":{"kept":"a"},"at":"end+0"},{"of":{"kept":"b"}}],"plus":40}]}"#
        ),
        Target::Point(Box::new([
            Axis::Of {
                target: Box::new(Target::Pane { dir: None }),
                at: Some(Offset::Pixels(2)),
                plus: 0,
            },
            Axis::Mid {
                pair: Box::new([
                    Axis::Of {
                        target: Box::new(Target::Kept("a".to_owned())),
                        at: Some(Offset::FromEnd(0)),
                        plus: 0,
                    },
                    Axis::Of {
                        target: Box::new(Target::Kept("b".to_owned())),
                        at: None,
                        plus: 0,
                    },
                ]),
                plus: 40,
            },
        ]))
    );
    assert_eq!(
        target(r#"{"entry":"todo.txt","part":"metadata"}"#),
        Target::Entry(EntryTarget {
            entry: "todo.txt".to_owned(),
            dir: None,
            part: Some(EntryPart::Metadata),
        })
    );
    for json in [
        r#"{"search_result":"todo.txt"}"#,
        r#"{"named":"Apply to All","within":{"dialog":true}}"#,
        r#"{"container":"archive"}"#,
        r#"{"dialog":true}"#,
        r#"{"parent":{"sidebar":"Home"}}"#,
        r#"{"ancestor":"scroll pane","of":{"pane":"deep"}}"#,
        r#"{"kept":"edge"}"#,
        r#"{"point":[{"extent":{"window":true},"plus":-4},{"extent":{"window":true},"plus":-4}]}"#,
        r#"{"point":[{"max":[{"of":{"pane":"a"},"at":10},{"of":{"viewport":null},"at":"4/5"}]},{"of":{"pane":"a"},"at":"50%"}]}"#,
        r#"{"role":"panel","within":{"entry":"a","dir":"b"}}"#,
        r#"{"role":"label","name":"b","description":"/a/b","within":{"toplevel":"Copy to"}}"#,
        r#"{"field":true,"within":{"toplevel":"Copy to"}}"#,
    ] {
        assert_eq!(
            target(json).to_json(),
            serde_json::from_str::<serde_json::Value>(json).expect("the JSON should parse"),
            "{json} should round-trip, since settle keys targets by their JSON"
        );
    }
    for bad in [
        r#"{"point":[{"of":{"pane":null}}]}"#,
        r#"{"point":[{"mid":[{"of":{"pane":null}}]},{"of":{"pane":null}}]}"#,
        r#"{"point":[{"mid":[{"of":{"pane":null}},{"of":{"pane":null}}],"at":2},{"of":{"pane":null}}]}"#,
        r#"{"point":[{"extent":{"window":true},"at":2},{"of":{"pane":null}}]}"#,
        r#"{"parent":{"sidebar":"Home"},"dir":"a"}"#,
        r#"{"dialog":false}"#,
        r#"{"field":"a","within":{"dialog":true}}"#,
        r#"{"toplevel":true}"#,
    ] {
        assert!(
            parse_command(&format!(r#"{{"move":{bad}}}"#)).is_err(),
            "{bad} should be rejected"
        );
    }
}

#[test]
fn new_commands_parse_and_reject_what_they_cannot_mean() {
    let Ok(Command::Wheel {
        until: Some(repeat),
        ..
    }) = parse_command(
        r#"{"wheel":{"viewport":null},"direction":"up","until":{"peek":false},"tries":40}"#,
    )
    else {
        panic!("a bounded wheel loop should parse");
    };
    assert_eq!((repeat.tries, repeat.message), (Some(40), None));
    let Ok(Command::Wheel {
        until: Some(repeat),
        ..
    }) = parse_command(r#"{"wheel":{"pane":null},"until":{"peek":false},"message":"m"}"#)
    else {
        panic!("a timed wheel loop should parse");
    };
    assert_eq!(
        (repeat.tries, repeat.timeout, repeat.message.as_deref()),
        (None, Duration::from_millis(15_000), Some("m"))
    );
    assert_eq!(
        parse_command(r#"{"during":{"peek":false},"ms":200,"every_ms":20,"message":"m"}"#),
        Ok(Command::During {
            checks: vec![(predicate(json!({ "peek": false })), Some("m".to_owned()))],
            duration: Duration::from_millis(200),
            every: Duration::from_millis(20),
        })
    );
    let Ok(Command::KeysUntil { keys, times, .. }) = parse_command(
        r#"{"keys_until":{"state":{"target":{"sidebar":"Home"},"focused":true}},"keys":["Home","Left"],"times":2}"#,
    ) else {
        panic!("a bounded key loop should parse");
    };
    assert_eq!((keys.len(), times), (2, 2));
    let Ok(Command::KeysUntil {
        times, check_first, ..
    }) = parse_command(
        r#"{"keys_until":{"state":{"target":{"menu_item":"Actions"},"focused":true}},"keys":["Down"],"times":["Cut","Copy","Actions"],"check_first":true}"#,
    )
    else {
        panic!("a key loop over a noted list should parse");
    };
    assert_eq!((times, check_first), (3, true));
    let Ok(Command::Expect { timeout, .. }) =
        parse_command(r#"{"expect":{"focused":"a"},"timeout_ms":4000,"as":"returned"}"#)
    else {
        panic!("a polling expect should parse");
    };
    assert_eq!(timeout, Some(Duration::from_millis(4000)));
    for bad in [
        r#"{"expect":{"mentions":{"target":{"dialog":true},"text":"a","words":"a b"}}}"#,
        r#"{"expect":{"file":{"path":"todo.txt","exists":true}}}"#,
        r#"{"wait":{"all":[]}}"#,
        r#"{"activate":{"background":true}}"#,
    ] {
        assert!(parse_command(bad).is_err(), "{bad} should be rejected");
    }
    let Ok(Command::Note { value, .. }) =
        parse_command(r#"{"note":"bounds","of":{"viewport":null},"as":"viewport_bounds"}"#)
    else {
        panic!("a bounds note should parse");
    };
    assert_eq!(value, Accessor::Bounds);
    assert_eq!(
        parse_command(r#"{"key_down":"ctrl"}"#),
        Ok(Command::KeyDown("ctrl".to_owned()))
    );
    assert_eq!(
        parse_command(r#"{"resize":{"width":600}}"#),
        Ok(Command::Resize {
            width: 600,
            height: None
        })
    );
    let Ok(Command::Note {
        value,
        middle,
        pick,
        ..
    }) = parse_command(r#"{"note":"visible","middle":3,"pick":-1,"as":"last"}"#)
    else {
        panic!("a shaped visible note should parse");
    };
    assert_eq!(
        (value, middle, pick),
        (Accessor::Visible, Some(3), Some(-1))
    );
    for bad in [
        r#"{"wheel":{"pane":null},"tries":3}"#,
        r#"{"wheel":{"pane":null},"until":{"peek":false},"tries":3,"timeout_ms":5}"#,
        r#"{"wheel":{"pane":null},"until":{"peek":false},"tries":0}"#,
        r#"{"during":{"peek":false},"ms":0}"#,
        r#"{"key_down":"ctrl+c"}"#,
        r#"{"note":"focused","pick":0,"as":"x"}"#,
        r#"{"note":"scroll_target","as":"x"}"#,
        r#"{"note":"bounds","as":"x"}"#,
        r#"{"keys_until":{"peek":true},"keys":[],"times":2}"#,
        r#"{"keys_until":{"peek":true},"keys":["Home"],"times":0}"#,
        r#"{"note":"entries","of":{"entry":"a"},"as":"x"}"#,
        r#"{"capture":{"kept":"a"},"as":""}"#,
        r#"{"wait":{"band_selected":{"from":"a"}}}"#,
    ] {
        assert!(parse_command(bad).is_err(), "{bad} should be rejected");
    }
}

#[test]
fn a_note_shapes_a_list_as_the_twin_slices_it() {
    let names: Vec<String> = ["010", "011", "012", "013", "014", "015", "016"]
        .map(str::to_owned)
        .to_vec();
    fn shape(middle: Option<usize>, plus: &[String], sorted: bool, pick: Option<i64>) -> Shape<'_> {
        Shape {
            from: None,
            through: None,
            middle,
            plus,
            sorted,
            pick,
        }
    }
    assert_eq!(
        shape(Some(3), &[], false, None).apply(names.clone()),
        Ok(json!(["013", "014", "015"]))
    );
    assert_eq!(
        shape(Some(3), &[], false, Some(-1)).apply(names.clone()),
        Ok(json!("015"))
    );
    assert_eq!(
        shape(
            None,
            &["todo.txt".to_owned(), "011".to_owned()],
            false,
            None
        )
        .apply(vec!["zz".to_owned(), "011".to_owned()]),
        Ok(json!(["011", "todo.txt", "zz"]))
    );
    assert_eq!(
        shape(None, &[], true, Some(0)).apply(vec!["b".to_owned(), "a".to_owned()]),
        Ok(json!("a"))
    );
    assert!(shape(None, &[], false, Some(9)).apply(names).is_err());
}

#[test]
fn name_filters_compare_names_as_python_strings_do() {
    let actual: Vec<String> = ["011.txt", "040.txt", "041.txt"]
        .map(str::to_owned)
        .to_vec();
    let names = |json: serde_json::Value| -> Names {
        serde_json::from_value(json).expect("the names should parse")
    };
    for holding in [
        json!({ "first": "011.txt" }),
        json!({ "first_above": "010.txt" }),
        json!({ "any_from": "040.txt" }),
        json!({ "toggled": { "names": ["011.txt", "000.txt"], "against": ["000.txt"] } }),
        json!({ "in_order": ["011.txt", "041.txt"] }),
        json!({ "set": ["041.txt", "011.txt", "040.txt"] }),
        json!({ "occurrences": { "011.txt": 1, "000.txt": 0 } }),
    ] {
        assert!(
            names(holding.clone()).matches(&actual),
            "{holding} should hold"
        );
    }
    for failing in [
        json!({ "first": "040.txt" }),
        json!({ "first_above": "011.txt" }),
        json!({ "any_from": "042.txt" }),
        json!({ "toggled": { "names": ["011.txt"], "against": ["011.txt"] } }),
        json!({ "in_order": ["041.txt", "011.txt"] }),
        json!({ "in_order": ["011.txt", "042.txt"] }),
        json!({ "set": ["011.txt", "040.txt"] }),
        json!({ "set": ["011.txt", "040.txt", "041.txt", "042.txt"] }),
        json!({ "occurrences": { "011.txt": 2 } }),
    ] {
        assert!(
            !names(failing.clone()).matches(&actual),
            "{failing} should not hold"
        );
    }
    assert!(Change::Exactly(0).matches(0));
    assert!(!Change::Exactly(0).matches(1));
    let grown = Change::Bounded {
        min: Some(30),
        max: None,
    };
    assert!(grown.matches(30) && !grown.matches(29));
}

fn capture(width: usize, height: usize, pixels: Vec<[u8; 3]>) -> Capture {
    Capture {
        x: 100,
        y: 50,
        width,
        height,
        pixels,
    }
}

#[test]
fn text_is_measured_like_the_pil_luma_centroid() {
    assert_eq!(pixels::luma([255, 255, 255]), 255);
    assert_eq!(pixels::luma([255, 0, 0]), 76);
    assert_eq!(pixels::luma([0, 255, 0]), 150);
    let white = [255, 255, 255];
    let black = [0, 0, 0];
    let mut rows = vec![white; 12];
    rows[8] = black;
    let (position, contrast) = pixels::text_position(&capture(4, 3, rows));
    assert_eq!((position, contrast), (Some(2.0), 255));
    let mut rows = vec![white; 12];
    rows[0] = black;
    rows[8] = black;
    assert_eq!(
        pixels::text_position(&capture(4, 3, rows)),
        (Some(1.0), 510)
    );
    assert_eq!(
        pixels::text_position(&capture(4, 3, vec![white; 12])),
        (None, 0),
        "a box with no ink has no position"
    );
}

#[test]
fn hover_feedback_ignores_only_the_pointer_square() {
    let before = capture(60, 40, vec![[10, 10, 10]; 2400]);
    let changed_at = |column: usize, row: usize| {
        let mut after = before.clone();
        after.pixels[row * 60 + column] = [200, 200, 200];
        after
    };
    let pointer = (100 + 10, 50 + 5);
    assert!(!pixels::differs_beside_pointer(
        &before,
        &changed_at(8, 3),
        pointer
    ));
    assert!(!pixels::differs_beside_pointer(
        &before,
        &changed_at(42, 37),
        pointer
    ));
    assert!(pixels::differs_beside_pointer(
        &before,
        &changed_at(7, 3),
        pointer
    ));
    assert!(pixels::differs_beside_pointer(
        &before,
        &changed_at(43, 10),
        pointer
    ));
    assert!(!pixels::differs_beside_pointer(&before, &before, pointer));
}

#[test]
fn the_theme_accent_is_read_from_the_catalog_entry_with_the_theme_id() {
    let catalog = "[[themes]]\nid = \"dark\"\naccent = \"#2ccceb\"\n\n\
                   [[themes]]\nid = \"light\"\naccent = \"#010203\"\n";
    assert_eq!(super::catalog_accent(catalog, "light"), Ok([1, 2, 3]));
    assert_eq!(
        super::catalog_accent(catalog, "dark"),
        Ok([0x2c, 0xcc, 0xeb])
    );
    assert!(super::catalog_accent(catalog, "missing").is_err());
    assert!(super::catalog_accent("[[themes]]\nid = \"x\"\naccent = \"teal\"\n", "x").is_err());
}

#[test]
fn screen_and_footer_predicates_parse_and_are_read_live() {
    for json in [
        json!({ "pixel": { "target": { "result": "beta/thumb.png" }, "rgb": [230, 40, 60] } }),
        json!({ "accent_pixels": { "target": { "field": "filter" }, "inset": 2, "more_than": 100 } }),
        json!({ "hit_path": { "mark": "search: txt", "hits": { "todo.txt": "todo.txt" }, "name": "todo.txt" } }),
        json!({ "focused_widget": { "contains": "Shortcuts" } }),
        json!({ "mentions": { "target": { "dialog": true }, "role": "label", "text": "permission" } }),
    ] {
        assert!(
            predicate(json.clone()).check.is_live(),
            "{json} reads the live window"
        );
    }
    let Check::FocusedWidget(Some(TextMatch::Like(pattern))) =
        predicate(json!({ "focused_widget": { "contains": "Shortcuts" } })).check
    else {
        panic!("a focused_widget pattern should parse as a text pattern");
    };
    assert_eq!(pattern.contains.as_deref(), Some("Shortcuts"));
}

#[test]
fn result_row_states_parse_and_need_a_state() {
    let Ok(Command::ClickResultRows { states, modifiers }) =
        parse_command(r#"{"click_result_rows":{"selected":true},"mods":["ctrl"]}"#)
    else {
        panic!("click_result_rows should parse");
    };
    assert!(states.matches(true, false) && !states.matches(false, true));
    assert_eq!(modifiers, vec![Modifier::Ctrl]);
    assert!(
        parse_command(r#"{"click_result_rows":{}}"#)
            .expect_err("an empty state should be rejected")
            .contains("needs a state")
    );
    assert!(
        predicate(json!({ "any_result_row": { "focused": true } }))
            .check
            .is_live()
    );
}

#[test]
fn numbers_compare_and_parse_from_their_operations() {
    use super::protocol::{CompareOp, Expr, Measure};
    assert!(CompareOp::AtMost.holds(2.0, 2.0));
    assert!(!CompareOp::Less.holds(2.0, 2.0));
    assert!(CompareOp::Unequal.holds(560.0, 412.0));
    let compare = predicate(json!({"compare": {
        "left": {"abs": {"minus": [{"of": {"preview": true}, "get": "x"}, {"of": {"pane": null}, "get": "right"}]}},
        "op": "<=",
        "right": 3,
    }}));
    let Check::Compare(comparison) = &compare.check else {
        panic!("compare parses as a comparison");
    };
    assert_eq!(comparison.op, CompareOp::AtMost);
    let Expr::Abs(inner) = &comparison.left else {
        panic!("abs wraps the difference");
    };
    let Expr::Minus(pair) = &**inner else {
        panic!("minus takes two numbers");
    };
    assert_eq!(
        pair[0],
        Expr::Of {
            target: Box::new(Target::Preview),
            get: Measure::X
        }
    );
    assert_eq!(
        serde_json::to_value(&compare).expect("a predicate serializes")["compare"]["left"],
        json!({"abs": {"minus": [{"of": {"preview": true}, "get": "x"}, {"of": {"pane": null}, "get": "right"}]}}),
        "a number serializes back to its spelling"
    );
    for bad in [
        json!({"compare": {"left": {"of": {"preview": true}}, "op": "<", "right": 1}}),
        json!({"compare": {"left": {"minus": [1]}, "op": "<", "right": 1}}),
        json!({"compare": {"left": {"pow": [1, 2]}, "op": "<", "right": 1}}),
        json!({"compare": {"left": 1, "op": "=<", "right": 1}}),
    ] {
        assert!(
            serde_json::from_value::<Predicate>(bad.clone()).is_err(),
            "{bad}"
        );
    }
    assert!(serde_json::from_value::<Predicate>(json!({"compare": {"left": {"count": {"description": "Columns view", "rendered": false}}, "op": "==", "right": {"pref": "text_size", "default": 0}}})).is_ok());
}

#[test]
fn prefs_panes_clipboard_holds_and_notes_parse_only_in_their_valid_shapes() {
    assert!(
        serde_json::from_value::<Predicate>(
            json!({"pref": {"key": "list_hidden_columns", "contains": "size"}})
        )
        .is_ok()
    );
    assert!(
        serde_json::from_value::<Predicate>(json!({"pref": {"key": "k", "value": null}})).is_ok()
    );
    for bad in [
        json!({"pref": {"key": "k"}}),
        json!({"pref": {"key": "k", "value": 1, "contains": "x"}}),
        json!({"panes_with": {"target": {"entry": "a"}, "names": []}}),
        json!({"panes_with": {"target": {"role": "button", "within": {"preview": true}}, "names": []}}),
        json!({"present": {"role": "label", "name_matches": "("}}),
        json!({"present": {"role": "label", "states": ["active"]}}),
    ] {
        assert!(
            serde_json::from_value::<Predicate>(bad.clone()).is_err(),
            "{bad}"
        );
    }
    assert_eq!(
        parse_command(r#"{"clipboard":{"starts_with":"alpha","longer_than":5},"message":"m"}"#),
        Ok(Command::Clipboard {
            text: serde_json::from_value(json!({"starts_with": "alpha", "longer_than": 5}))
                .expect("the json should parse"),
            message: Some("m".to_owned()),
        })
    );
    assert_eq!(
        parse_command(r#"{"clipboard_set":"selection-cleared"}"#),
        Ok(Command::ClipboardSet("selection-cleared".to_owned()))
    );
    assert!(parse_command(r#"{"expect":{"focused":null},"hold_ms":400}"#).is_err());
    assert!(
        parse_command(r#"{"expect":{"focused":null},"timeout_ms":3000,"hold_ms":400}"#).is_ok()
    );
    assert!(parse_command(r#"{"note":"number","as":"n"}"#).is_err());
    assert!(parse_command(r#"{"note":"entries","expr":1,"as":"n"}"#).is_err());
    assert!(
        parse_command(r#"{"note":"number","expr":{"of":{"window":true},"get":"width"},"as":"n"}"#)
            .is_ok()
    );
    assert!(parse_command(r#"{"note":"visible_top","pick":0,"as":"n"}"#).is_ok());
    assert!(parse_command(
        r#"{"note":"number","expr":{"pref":"w"},"until":[{"op":">","than":300}],"until_found":true,"as":"n"}"#
    )
    .is_ok());
    assert!(
        parse_command(
            r#"{"note":"number","expr":{"pref":"w"},"until":[{"op":">","than":300}],"as":"n"}"#
        )
        .is_err(),
        "until needs until_found"
    );
    let longer: TextMatch =
        serde_json::from_value(json!({"starts_with": "alpha", "longer_than": 5}))
            .expect("the json should parse");
    assert!(longer.matches("alpha bravo"));
    assert!(!longer.matches("alpha"));
    assert!(!longer.matches("bravo alpha"));
}

#[test]
fn role_targets_carry_states_and_name_patterns() {
    let target: Target = serde_json::from_value(
        json!({"role": "page tab", "name": "docs", "states": ["selected"], "name_matches": "^d"}),
    )
    .expect("the json should parse");
    assert_eq!(
        target.to_json(),
        json!({"role": "page tab", "name": "docs", "states": ["selected"], "name_matches": "^d"})
    );
}

#[test]
fn batch_six_commands_parse_only_in_their_valid_shapes() {
    let target: Target =
        serde_json::from_value(json!({"role": ["check box", "toggle button"], "name": "Confirm"}))
            .expect("a role list should parse");
    assert_eq!(
        target.to_json(),
        json!({"role": ["check box", "toggle button"], "name": "Confirm"})
    );
    assert_eq!(
        serde_json::from_value::<Target>(json!({"other_window": "Strata"})).ok(),
        Some(Target::OtherWindow("Strata".to_owned()))
    );
    assert!(serde_json::from_value::<Target>(json!({"role": [], "name": "x"})).is_err());
    let Ok(Command::Wait {
        never: Some(never), ..
    }) = parse_command(r#"{"wait":{"preview":true},"never":{"preview":null},"never_message":"m"}"#)
    else {
        panic!("a wait with never should parse");
    };
    assert_eq!(never.1.as_deref(), Some("m"));
    let Ok(Command::During { checks, .. }) = parse_command(
        r#"{"during":[{"check":{"preview":true},"message":"a"},{"check":{"peek":false}}],"ms":100}"#,
    ) else {
        panic!("a during list should parse");
    };
    assert_eq!(checks.len(), 2);
    for invalid in [
        r#"{"wait":{"preview":true},"never_message":"m"}"#,
        r#"{"during":[],"ms":100}"#,
        r#"{"during":[{"check":{"peek":false}}],"ms":100,"message":"m"}"#,
        r#"{"wait":{"largest_image":{"within":{"preview":true},"size":[1,1],"exceeds":[1,1]}}}"#,
        r#"{"note":"entries","as":"n","until_names":{"at_least":2}}"#,
        r#"{"wait":{"pref":{"key":"k","value":1,"one_of":[1]}}}"#,
        r#"{"wait":{"mentions":{"target":{"dialog":true},"text":"a","equals":"a"}}}"#,
    ] {
        assert!(
            parse_command(invalid).is_err(),
            "{invalid} should be rejected"
        );
    }
}

#[test]
fn notebook_tabs_and_scrolled_text_report_what_the_bridge_does() {
    crate::test_support::gtk_test(
        "qa::tests::notebook_tabs_and_scrolled_text_report_what_the_bridge_does",
        || {
            use gtk::prelude::*;
            let notebook = gtk::Notebook::new();
            for title in ["General", "Script"] {
                notebook.append_page(
                    &gtk::Box::new(gtk::Orientation::Vertical, 0),
                    Some(&gtk::Label::new(Some(title))),
                );
            }
            notebook.set_current_page(Some(1));
            let selected: Vec<(String, bool)> = super::targets::presented(notebook.upcast_ref())
                .iter()
                .filter(|widget| super::targets::role_name(widget) == "page tab")
                .map(|tab| {
                    (
                        super::targets::accessible_name(tab),
                        super::targets::states(tab).selected,
                    )
                })
                .collect();
            assert_eq!(
                selected,
                [("General".to_owned(), false), ("Script".to_owned(), true)]
            );

            let view = gtk::TextView::new();
            let scroll = gtk::ScrolledWindow::builder().child(&view).build();
            crate::ui::accessibility_qa::record(&scroll, |naming| {
                naming.label = Some("Arguments".to_owned())
            });
            assert_eq!(
                super::targets::accessible_name(view.upcast_ref()),
                "Arguments"
            );
            assert_eq!(super::targets::accessible_name(scroll.upcast_ref()), "");
            assert!(super::targets::states(view.upcast_ref()).editable);
            let plain = gtk::TextView::new();
            let _unlabelled = gtk::ScrolledWindow::builder().child(&plain).build();
            assert_eq!(super::targets::accessible_name(plain.upcast_ref()), "");
        },
    );
}

#[test]
fn portal_reveal_and_window_steps_parse_and_reject_what_they_cannot_mean() {
    assert_eq!(
        parse_command(r#"{"adopt":"Picker","timeout_ms":20000,"message":"m"}"#),
        Ok(Command::Adopt {
            title: "Picker".to_owned(),
            timeout: Duration::from_millis(20_000),
            message: Some("m".to_owned()),
        })
    );
    assert_eq!(
        parse_command(r#"{"keep":[1,2,3,4],"as":"foreign"}"#),
        Ok(Command::Keep {
            rect: [1, 2, 3, 4],
            name: "foreign".to_owned(),
        })
    );
    assert!(matches!(
        parse_command(
            r#"{"keys_until":{"focused":true},"keys":["Tab"],"times":3,"check_first":true,"await_focus_change":{"timeout_ms":5000},"exhausted":"never"}"#
        ),
        Ok(Command::KeysUntil {
            await_focus_change: Some((timeout, None)),
            exhausted: Some(_),
            ..
        }) if timeout == Duration::from_millis(5000)
    ));
    assert!(matches!(
        parse_command(r#"{"open_menu":{"role":"button","name":"A"},"until":{"menu_items":{}}}"#),
        Ok(Command::OpenMenu { open_states, .. }) if open_states == [StateName::Checked]
    ));
    for invalid in [
        r#"{"adopt":""}"#,
        r#"{"keep":[1,2,3],"as":"x"}"#,
        r#"{"reveal":{"role":"button"}}"#,
        r#"{"reveal_control":{"entry":"a"}}"#,
        r#"{"open_menu":{"role":"button","name":"A"},"until":{"menu_items":{}},"open_states":[]}"#,
        r#"{"wait":{"file":{"path":"/a","exists":true,"mode_bits":{"mask":"100","set":true}}}}"#,
        r#"{"wait":{"file":{"path":"/a","mode_bits":{"mask":"9","set":true}}}}"#,
        r#"{"wait":{"present":{"role":["*","button"]}}}"#,
        r#"{"wait":{"present":{"role":"list item","has":{"entry":"a"}}}}"#,
    ] {
        assert!(
            parse_command(invalid).is_err(),
            "{invalid} should be rejected"
        );
    }
    for valid in [
        r#"{"wait":{"some_window":{"title":"Strata","holds":{"windows":{"title":"Strata","count":2}}}}}"#,
        r#"{"wait":{"child_names":{"of":{"parent":{"sidebar":"Home"}},"role":"button","names":{"starts_with":["Home"]}}}}"#,
        r#"{"wait":{"present":{"role":"*","states":["selected"],"within":{"preview":true}}}}"#,
        r#"{"wait":{"present":{"role":"list item","has":{"role":"label","name":"a"}}}}"#,
        r#"{"wait":{"state":{"target":{"row":"a"},"focusable":true}}}"#,
        r#"{"expect":[{"alive":"x"},{"focus_outside":{"remembered":"x"}},{"focused_widget_is":{"remembered":"x"}}]}"#,
        r#"{"expect":{"subtree_text":{"target":{"dialog":true},"contains":"a","strip_newlines":true}}}"#,
    ] {
        assert!(parse_command(valid).is_ok(), "{valid} should parse");
    }
}

#[test]
fn name_filters_take_a_leading_run() {
    let names: Vec<String> = ["Home", "Trash", "Network"].map(str::to_owned).into();
    let leading = |first: &[&str]| {
        Names::Including(Box::new(
            serde_json::from_value(json!({ "starts_with": first })).expect("a name filter"),
        ))
    };
    assert!(leading(&["Home", "Trash"]).matches(&names));
    assert!(!leading(&["Trash", "Home"]).matches(&names));
}

#[test]
fn buttons_alerts_passwords_and_list_rows_report_what_the_bridge_does() {
    crate::test_support::gtk_test(
        "qa::tests::buttons_alerts_passwords_and_list_rows_report_what_the_bridge_does",
        || {
            use gtk::prelude::*;
            let name = |widget: &gtk::Widget| super::targets::accessible_name(widget);
            let labelled = gtk::Button::with_label("Home directory");
            crate::ui::accessibility_qa::record(&labelled, |naming| {
                naming.label = Some("Default directory".to_owned());
            });
            assert_eq!(name(labelled.upcast_ref()), "Home directory");
            let reset = gtk::Button::with_label("r");
            crate::ui::accessibility_qa::record_labelled_by_reset(&reset);
            crate::ui::accessibility_qa::record(&reset, |naming| {
                naming.label = Some("Owner read".to_owned());
            });
            assert_eq!(name(reset.upcast_ref()), "Owner read");
            let icon = gtk::Button::from_icon_name("edit-find");
            crate::ui::accessibility_qa::record(&icon, |naming| {
                naming.label = Some("Search".to_owned());
            });
            assert_eq!(name(icon.upcast_ref()), "Search");

            let alert = gtk::Label::builder()
                .accessible_role(gtk::AccessibleRole::Alert)
                .build();
            assert_eq!(super::targets::role_name(alert.upcast_ref()), "alert");
            let password = gtk::PasswordEntry::new();
            assert_eq!(
                super::targets::role_name(password.upcast_ref()),
                "password text"
            );

            let row = gtk::Box::builder()
                .accessible_role(gtk::AccessibleRole::ListItem)
                .build();
            assert!(!super::targets::states(row.upcast_ref()).selected);
            row.set_state_flags(gtk::StateFlags::SELECTED, false);
            assert!(super::targets::states(row.upcast_ref()).selected);
            let plain = gtk::Box::new(gtk::Orientation::Vertical, 0);
            plain.set_state_flags(gtk::StateFlags::SELECTED, false);
            assert!(!super::targets::states(plain.upcast_ref()).selected);
        },
    );
}
