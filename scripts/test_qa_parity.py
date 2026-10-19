# SPDX-License-Identifier: MIT

import contextlib
import io
import json
import os
import re
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import qa_parity
from qa_parity import (
    QaRun,
    Result,
    RunPair,
    compare_input,
    compare_state,
    exit_accounted,
    fail_parity,
    input_parity_ignore,
    parse_junit,
    parse_qa_log,
    pass_parity,
    qa_signature,
    shard_units,
    signature_parity,
    skip_reason,
    state_parity_ignore,
    touches_parity,
)

REPOSITORY = Path(__file__).resolve().parents[1]

TWIN = "tests/e2e/scenarios/test_clipboard.py::test_copy_leaves_the_source_in_place"


class QaLogTests(unittest.TestCase):
    def test_runs_map_to_twins_through_their_result_lines(self):
        log = "\n".join([
            f"PASS copy-leaves-source/columns (twin {TWIN}[columns])",
            "thread 'copy_leaves_source' panicked at tests/qa.rs:1:1:",
            f"copy-leaves-source/icons (twin {TWIN}[icons]) failed:",
            "  line 4 {...}: timed out",
            f"copy-leaves-source/list (twin {TWIN}[list]) could not run:",
            "  launch: no window",
        ])
        runs = parse_qa_log(log)
        self.assertEqual(
            {twin: (run.run, run.result.status) for twin, run in runs.items()},
            {
                f"{TWIN}[columns]": ("columns", "pass"),
                f"{TWIN}[icons]": ("icons", "fail"),
                f"{TWIN}[list]": ("list", "error"),
            },
        )


    def test_a_planned_run_without_a_result_is_missing_not_dropped(self):
        log = "\n".join([
            f"RUN copy-leaves-source/columns (twin {TWIN}[columns])",
            f"RUN copy-leaves-source/icons (twin {TWIN}[icons])",
            f"RUN copy-leaves-source/list (twin {TWIN}[list])",
            f"PASS copy-leaves-source/columns (twin {TWIN}[columns])",
            f"KNOWN BUG copy-leaves-source/list (twin {TWIN}[list]) still fails as expected (#1)",
            "error: test failed, to rerun pass `--test qa`",
        ])
        runs = parse_qa_log(log)
        self.assertEqual(
            [runs[f"{TWIN}[{mode}]"].result.status for mode in ("columns", "icons", "list")],
            ["pass", "missing", "known bug"],
        )
        found = [RunPair(twin, run.run, run.result, Result("pass")) for twin, run in runs.items()]
        self.assertFalse(pass_parity(found).agree)
        failing = [RunPair(twin, run.run, run.result, Result("fail")) for twin, run in runs.items()]
        self.assertFalse(fail_parity(failing).agree)

    def test_the_exit_status_must_match_the_run_results(self):
        passing = [QaRun("s", "columns", "t", Result("pass"))]
        failing = [QaRun("s", "columns", "t", Result("fail"))]
        self.assertTrue(exit_accounted(0, passing).agree)
        self.assertTrue(exit_accounted(101, failing).agree)
        self.assertFalse(exit_accounted(101, passing).agree)
        self.assertFalse(exit_accounted(0, failing).agree)
        self.assertFalse(exit_accounted(101, []).agree)
        self.assertFalse(exit_accounted(None, passing).agree)


class SignatureTests(unittest.TestCase):
    def test_python_wait_messages_and_qa_step_messages_are_compared(self):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "junit.xml"
            report.write_text(
                "<testsuites><testsuite>"
                '<testcase classname="tests.e2e.scenarios.test_clipboard" name="test_copy_leaves_the_source_in_place[columns]">'
                '<failure message="harness.tree.TreeTimeout: timed out after 15s waiting for the copy to land in archive&#10;tree"/>'
                "</testcase>"
                '<testcase classname="tests.e2e.scenarios.test_clipboard" name="test_copy_leaves_the_source_in_place[icons]">'
                '<failure message="AssertionError: cut must not touch the filesystem&#10;assert False"/>'
                "</testcase>"
                '<testcase classname="tests.e2e.scenarios.test_clipboard" name="test_copy_leaves_the_source_in_place[list]">'
                '<error message="display did not start"/>'
                "</testcase>"
                '<testcase classname="tests.e2e.scenarios.test_clipboard" name="test_copy_leaves_the_source_in_place[a@b]@visual-baselines"/>'
                '<testcase classname="tests.e2e.scenarios.test_clipboard" name="test_copy_leaves_the_source_in_place[c@d]"/>'
                "</testsuite></testsuites>"
            )
            results = parse_junit(report)
        self.assertEqual(results[f"{TWIN}[columns]"].signature, "the copy to land in archive")
        self.assertEqual(results[f"{TWIN}[icons]"].signature, "cut must not touch the filesystem")
        self.assertEqual(results[f"{TWIN}[list]"].status, "error")
        self.assertEqual(results[f"{TWIN}[columns]"].node, "")
        grouped = results[f"{TWIN}[a@b]"]
        self.assertEqual((grouped.status, grouped.node), ("pass", f"{TWIN}[a@b]@visual-baselines"))
        bare = results[f"{TWIN}[c@d]"]
        self.assertEqual((bare.status, bare.node), ("pass", ""))

        signature, where = qa_signature({
            "failures": [{
                "line": 9,
                "step": {"fs": {"path": "archive/todo.txt", "exists": True}, "message": "the copy to land in archive"},
                "text": "line 9 ...: timed out waiting for the copy to land in archive",
            }]
        })
        self.assertEqual(signature, "the copy to land in archive")
        self.assertTrue(where.startswith("line 9: "))

    def test_runs_failing_at_different_steps_disagree(self):
        same = RunPair("a", "s/columns", Result("fail", "the copy"), Result("fail", "the copy"))
        other = RunPair("b", "s/icons", Result("fail", "a dialog to appear"), Result("fail", "the copy"))
        self.assertTrue(signature_parity([same]).agree)
        self.assertFalse(signature_parity([same, other]).agree)
        unnamed = RunPair("c", "s/list", Result("fail", ""), Result("fail", ""))
        self.assertFalse(signature_parity([unnamed]).agree)


class FailParityTests(unittest.TestCase):
    def pair(self, qa, python):
        return RunPair("twin", f"s/{qa}-{python}", Result(qa), Result(python))

    def test_each_run_must_fail_on_both_sides_or_neither(self):
        cases = [
            ([("fail", "fail"), ("pass", "pass")], True),
            ([("fail", "fail"), ("fail", "pass")], False),
            ([("pass", "fail")], False),
            ([("pass", "pass")], False),
            ([("fail", "error")], False),
            ([("fail", "missing")], False),
        ]
        for statuses, expected in cases:
            with self.subTest(statuses=statuses):
                found = [self.pair(qa, python) for qa, python in statuses]
                self.assertEqual(fail_parity(found).agree, expected)


class InputParityTests(unittest.TestCase):
    QA = "\n".join([
        "motion 94 194",
        "motion 100 200",
        "pointer 100 200 button 1 down",
        "pointer 100 200 button 1 up",
        "key 37 down",
        "key 37 up",
    ])

    def test_pointer_buttons_match_within_one_pixel_and_motion_is_ignored(self):
        python = self.QA.replace("motion 94 194\n", "").replace("100 200 button 1 up", "101 199 button 1 up")
        self.assertTrue(compare_input(python, self.QA).agree)

    def test_a_larger_offset_reordered_keys_or_extra_events_disagree(self):
        for python in [
            self.QA.replace("100 200 button 1 down", "102 200 button 1 down"),
            self.QA.replace("key 37 down\nkey 37 up", "key 37 up\nkey 37 down"),
            self.QA.replace("button 1 down", "button 3 down"),
            self.QA + "\nkey 50 down",
            self.QA + "\nscroll 1 2",
        ]:
            with self.subTest(python=python):
                self.assertFalse(compare_input(python, self.QA).agree)

    def test_a_motion_only_run_compares_its_motion_and_an_empty_one_disagrees(self):
        hover = "motion 1190 750\nmotion 1196 756\nmotion 267 152\nmotion 273 158"
        self.assertTrue(compare_input(hover, hover.replace("273 158", "274 157")).agree)
        self.assertFalse(compare_input(hover, hover.replace("273 158", "276 158")).agree)
        self.assertFalse(compare_input(hover, hover.rsplit("\n", 1)[0]).agree)
        self.assertFalse(compare_input("", "").agree)
        self.assertTrue(compare_input("", "", sends_input=False).agree)
        long = "tests/e2e/scenarios/test_column_headers.py::test_titles[" + "x" * 300 + "]"
        name = qa_parity.node_file_name(long)
        self.assertLessEqual(len(name), qa_parity.NODE_FILE_NAME_LIMIT)
        self.assertNotEqual(name, qa_parity.node_file_name(long.replace("x]", "y]")))
        self.assertEqual(qa_parity.node_file_name("a::b[c]"), "a_b_c_")
        self.assertFalse(compare_input("", "motion 1 2\n", sends_input=False).agree)


class StateParityIgnoreTests(unittest.TestCase):
    PYTHON = {
        "view_mode": "Columns",
        "directory": "root",
        "panes": [{"name": "root", "selected": [], "focused": "todo.txt"}],
        "focused": "todo.txt",
        "focused_pane": "root",
        "dialog": None,
    }

    def test_ignored_focus_is_left_out_and_everything_else_still_compared(self):
        qa = {
            **self.PYTHON,
            "panes": [{"name": "root", "selected": [], "focused": "readme.md"}],
            "focused": "readme.md",
        }
        ignore = ("focused", "panes.focused")
        self.assertFalse(compare_state(self.PYTHON, qa).agree)
        self.assertTrue(compare_state(self.PYTHON, qa, ignore).agree)
        self.assertFalse(compare_state(self.PYTHON, {**qa, "directory": "home"}, ignore).agree)
        moved = {**qa, "panes": [{"name": "root", "selected": ["a"], "focused": None}]}
        self.assertFalse(compare_state(self.PYTHON, moved, ignore).agree)

    def test_only_distinct_compared_keys_may_be_ignored(self):
        self.assertEqual(state_parity_ignore({}), ())
        self.assertEqual(
            state_parity_ignore({"state_parity_ignore": ["focused", "panes.focused"]}),
            ("focused", "panes.focused"),
        )
        for bad in [["menu"], ["panes.name"], ["focused", "focused"], "focused", [1]]:
            with self.subTest(bad=bad):
                with self.assertRaises(ValueError):
                    state_parity_ignore({"state_parity_ignore": bad})

    def test_input_parity_may_ignore_named_pointer_positions_of_one_run(self):
        down = {"position": 19, "event": "button 1 down"}
        up = {"position": 20, "event": "button 1 up"}
        header = {
            "modes": ["columns"],
            "variants": [{"id": "wheel"}, {"id": "reset"}],
            "input_parity_ignore": [{"run": "columns-wheel", "events": [down, up]}],
        }
        self.assertEqual(
            input_parity_ignore(header),
            (("columns-wheel", ((19, "button 1 down"), (20, "button 1 up"))),),
        )
        self.assertEqual(input_parity_ignore({"modes": ["columns"]}), ())
        for bad in [
            [{"run": "columns", "events": [down]}],
            [{"run": "columns-wheel", "events": []}],
            [{"run": "columns-wheel", "events": [{"position": 0, "event": "button 1 down"}]}],
            [{"run": "columns-wheel", "events": [down, down]}],
            [{"run": "columns-wheel", "events": [{"position": True, "event": "button 1 down"}]}],
            [{"run": "columns-wheel", "events": [{"position": 2, "event": "key 37 down"}]}],
            [{"run": "columns-wheel", "events": [{"position": 2}]}],
            [{"run": "columns-wheel", "events": [down], "why": "x"}],
            [{"run": "columns-wheel", "positions": [19]}],
            [{"run": "columns-wheel", "events": [down]}, {"run": "columns-wheel", "events": [up]}],
            {"run": "columns-wheel", "events": [down]},
        ]:
            with self.subTest(bad=bad):
                with self.assertRaises(ValueError):
                    input_parity_ignore({**header, "input_parity_ignore": bad})
        with self.assertRaises(ValueError):
            input_parity_ignore({**header, "mutation": "quick-preview"})

    def test_ignored_positions_must_hold_the_named_events_and_the_rest_still_compare(self):
        python = "key 37 down\npointer 968 416 button 1 down\npointer 968 416 button 1 up\nkey 37 up\n"
        qa = "key 37 down\npointer 1035 387 button 1 down\npointer 1035 387 button 1 up\nkey 37 up\n"
        click = ((2, "button 1 down"), (3, "button 1 up"))
        self.assertFalse(compare_input(python, qa).agree)
        self.assertTrue(compare_input(python, qa, positions=click).agree)
        self.assertFalse(compare_input(python, qa, positions=click[:1]).agree)
        released = qa.replace("button 1 down", "button 1 up", 1)
        self.assertFalse(compare_input(python, released, positions=click).agree)
        self.assertFalse(
            compare_input(python, qa, positions=((2, "button 3 down"), (3, "button 1 up"))).agree,
            "the event at the position must be the one named",
        )
        shifted = "key 37 down\n" + qa
        self.assertFalse(compare_input("key 37 down\n" + python, shifted, positions=click).agree)
        self.assertFalse(
            compare_input(python, qa, positions=click + ((9, "button 1 down"),)).agree,
            "a position past either log is rejected",
        )
        self.assertFalse(compare_input(python, python, positions=((1, "button 1 down"),)).agree)
        everything = ((1, "button 1 down"), (2, "button 1 down"), (3, "button 1 up"), (4, "button 1 up"))
        self.assertFalse(compare_input(python, python, positions=everything).agree)

    def test_input_parity_may_ignore_named_key_events_and_typed_digits(self):
        header = {"modes": ["list"], "input_parity_ignore": [{"run": "list", "events": [
            {"position": 3, "event": "key down"},
            {"position": 4, "event": "key up"},
            {"position": 9, "event": "typed digits"},
        ]}]}
        self.assertEqual(
            input_parity_ignore(header),
            (("list", ((3, "key down"), (4, "key up"), (9, "typed digits"))),),
        )
        for bad in ["key", "key 12 down", "typed", "typed digits down"]:
            with self.subTest(bad=bad):
                with self.assertRaises(ValueError):
                    input_parity_ignore({**header, "input_parity_ignore": [
                        {"run": "list", "events": [{"position": 1, "event": bad}]}]})

    def test_ignored_keys_compare_only_their_direction(self):
        python = "key 36 down\nkey 36 up\nkey 12 down\nkey 12 up\nkey 61 down\nkey 61 up\n"
        qa = "key 36 down\nkey 36 up\nkey 15 down\nkey 15 up\nkey 61 down\nkey 61 up\n"
        port = ((3, "key down"), (4, "key up"))
        self.assertFalse(compare_input(python, qa).agree)
        self.assertTrue(compare_input(python, qa, positions=port).agree)
        self.assertFalse(compare_input(python, qa, positions=port[:1]).agree)
        self.assertFalse(
            compare_input(python, qa, positions=((3, "key up"), (4, "key up"))).agree,
            "the direction is still compared",
        )
        clicked = qa.replace("key 15 down", "pointer 1 1 button 1 down")
        self.assertFalse(compare_input(python, clicked, positions=port).agree)

    def test_typed_digits_may_differ_in_length_and_the_rest_still_compare(self):
        before = "key 36 down\nkey 36 up\n"
        after = "pointer 40 50 button 1 down\npointer 40 50 button 1 up\nkey 36 down\nkey 36 up\n"
        python = before + "key 12 down\nkey 12 up\n" + after
        qa = before + "key 14 down\nkey 14 up\nkey 19 down\nkey 19 up\nkey 11 down\nkey 11 up\n" + after
        typed = ((3, "typed digits"),)
        self.assertFalse(compare_input(python, qa).agree)
        self.assertTrue(compare_input(python, qa, positions=typed).agree)
        self.assertFalse(
            compare_input(python, qa.replace("pointer 40 50", "pointer 90 50"), positions=typed).agree,
            "events after the digits are still compared",
        )
        lettered = before + "key 38 down\nkey 38 up\n" + after
        self.assertFalse(compare_input(python, lettered, positions=typed).agree, "a letter is no digit")
        self.assertFalse(compare_input(python, qa, positions=((1, "typed digits"),)).agree)
        d = lambda *codes: "".join(f"key {code} down\nkey {code} up\n" for code in codes)  # noqa: E731
        self.assertFalse(
            compare_input(d(12, 13) + d(36), d(14) + d(36), positions=((1, "typed digits"), (2, "button 1 down"))).agree,
            "a named position inside the digits is never skipped unchecked",
        )
        self.assertFalse(
            compare_input(d(12, 13), d(14), positions=((1, "typed digits"),)).agree,
            "typed digits over every event leave nothing compared",
        )
        self.assertTrue(compare_input(d(12, 13) + d(36), d(14) + d(36), positions=((1, "typed digits"),)).agree)

    def test_a_skipped_scenario_gives_a_reason_and_leaves_parity(self):
        self.assertIsNone(skip_reason({"modes": ["columns"]}))
        self.assertEqual(skip_reason({"modes": ["columns"], "skip": "issue 9"}), "issue 9")
        for bad in [{"skip": ""}, {"skip": 3}, {"skip": "x", "mutation": "clipboard"}, {"skip": "x", "known_bug": "y"}]:
            with self.subTest(bad=bad):
                with self.assertRaises(ValueError):
                    skip_reason({"modes": ["columns"], **bad})
        skipped = [scenario for scenario in qa_parity.scenarios() if scenario.skip]
        self.assertTrue(skipped, "the text-size scenario is skipped until issue 9 is fixed")
        self.assertFalse({scenario.name for scenario in skipped} & set(qa_parity.ci_units()))
        self.assertFalse({scenario.name for scenario in skipped} & {s.name for s in qa_parity.chosen_scenarios([])})
        self.assertTrue(all(
            line.startswith(("Skipped, not compared: ", "No twin, not compared: "))
            for line in qa_parity.skipped_lines([])
        ))

    def test_a_mutation_scenario_or_ignoring_every_key_is_rejected(self):
        self.assertEqual(state_parity_ignore({"mutation": "clipboard"}), ())
        with self.assertRaises(ValueError):
            state_parity_ignore({"mutation": "clipboard", "state_parity_ignore": ["focused"]})
        with self.assertRaises(ValueError):
            state_parity_ignore({"state_parity_ignore": list(qa_parity.COMPARED_STATE)})
        everything_but_pane_names = [
            key for key in qa_parity.IGNORABLE_STATE if key != "panes"
        ]
        self.assertEqual(
            state_parity_ignore({"state_parity_ignore": everything_but_pane_names}),
            tuple(everything_but_pane_names),
        )

    def test_the_whole_state_may_be_left_out_alone_and_only_without_a_mutation(self):
        header = {"modes": ["columns"], "state_parity_ignore": ["*"]}
        self.assertEqual(state_parity_ignore(header), ("*",))
        with self.assertRaises(ValueError):
            state_parity_ignore({**header, "state_parity_ignore": ["*", "focused"]})
        with self.assertRaises(ValueError):
            state_parity_ignore({**header, "mutation": "clipboard"})
        gone = {"error": "AssertionError('the Strata application is not on the a11y bus')"}
        self.assertTrue(compare_state(gone, {"directory": "x"}, ("*",)).agree)
        self.assertFalse(compare_state(gone, {"directory": "x"}).agree)

    def test_the_note_names_the_ignored_keys(self):
        qa = {**self.PYTHON, "focused": "readme.md"}
        self.assertEqual(
            compare_state(self.PYTHON, qa, ("focused",)).note, "ignoring focused"
        )


class StateParityTests(unittest.TestCase):
    def test_only_the_compared_keys_must_match(self):
        state = {
            "view_mode": "Columns",
            "directory": "archive",
            "panes": [{"name": "archive", "selected": ["todo.txt"], "focused": "todo.txt"}],
            "focused": "todo.txt",
            "focused_pane": "archive",
            "dialog": None,
        }
        qa = {**state, "menu": None, "preview": ["todo"], "field": None, "matches": []}
        self.assertTrue(compare_state(state, qa).agree)
        self.assertFalse(compare_state({**state, "focused": None}, qa).agree)
        self.assertFalse(compare_state({"error": "gone"}, qa).agree)
        without_dialog = {key: value for key, value in state.items() if key != "dialog"}
        self.assertFalse(compare_state(without_dialog, {**without_dialog, "menu": None}).agree)


class CiShardingTests(unittest.TestCase):
    def test_every_scenario_lands_in_exactly_one_shard_by_its_patch_or_name(self):
        units = qa_parity.ci_units()
        found = qa_parity.scenarios()
        self.assertEqual(
            sorted(units),
            sorted(scenario.mutation or scenario.name for scenario in found
                   if scenario.skip is None and scenario.twin),
        )
        for shards in (1, 3, qa_parity.CI_SHARDS, len(units) + 5):
            with self.subTest(shards=shards):
                assigned = shard_units(units, shards)
                self.assertEqual(len(assigned), min(shards, len(units)))
                self.assertTrue(all(assigned))
                self.assertEqual(sorted(sum(assigned, [])), units)

    def test_the_plan_accepts_scenarios_without_a_mutation(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            scenarios, mutations = root / "scenarios", root / "mutations"
            (scenarios / "test_example").mkdir(parents=True)
            mutations.mkdir()
            (scenarios / "test_example" / "alpha.jsonl").write_text(json.dumps({"scenario": "alpha", "twin": "x", "mutation": "break-alpha"}) + "\n")
            (scenarios / "test_example" / "beta.jsonl").write_text(json.dumps({"scenario": "beta", "twin": "x", "mutation": None}) + "\n")
            (mutations / "break-alpha.patch").write_text("+++ b/src/main.rs\n")
            with mock.patch.object(qa_parity, "SCENARIOS", scenarios), \
                    mock.patch.object(qa_parity, "MUTATIONS", mutations), \
                    mock.patch.object(qa_parity, "CI_SHARDS", 1):
                self.assertEqual(qa_parity.ci_units(), ["beta", "break-alpha"])
                output = io.StringIO()
                with contextlib.redirect_stdout(output):
                    self.assertEqual(qa_parity.ci_plan(None), 0)
                self.assertIn('"names":"beta break-alpha"', output.getvalue())
                (scenarios / "test_example" / "break-alpha.jsonl").write_text(json.dumps({"scenario": "break-alpha", "twin": "x", "mutation": None}) + "\n")
                with self.assertRaises(SystemExit):
                    qa_parity.ci_units()

    def test_pull_requests_run_parity_only_when_either_twin_can_change(self):
        cases = [
            (["src/ui/browser/mod.rs"], True),
            (["tests/e2e/scenarios/test_clipboard.py"], True),
            (["tests/qa/scenarios/test_clipboard/copy-leaves-source.jsonl"], True),
            (["tests/e2e/mutations/clipboard.patch"], True),
            (["tests/qa.rs"], True),
            (["docs/e2e-testing.md", "README.md"], False),
            (["srcery/main.rs", "tests/qa.rs.orig"], False),
            ([], False),
        ]
        for paths, expected in cases:
            with self.subTest(paths=paths):
                self.assertEqual(touches_parity(paths), expected)


class ScenarioChoiceTests(unittest.TestCase):
    def test_a_name_is_a_mutation_patch_or_else_a_scenario(self):
        with tempfile.TemporaryDirectory() as directory:
            scenarios = Path(directory)
            (scenarios / "test_example").mkdir()
            (scenarios / "test_example" / "alpha.jsonl").write_text(json.dumps({"scenario": "alpha", "twin": "x", "mutation": "break-alpha"}) + "\n")
            (scenarios / "test_example" / "beta.jsonl").write_text(json.dumps({"scenario": "beta", "twin": "x", "mutation": None}) + "\n")
            with mock.patch.object(qa_parity, "SCENARIOS", scenarios):
                chosen = qa_parity.chosen_scenarios(["break-alpha", "beta", "alpha"])
                self.assertEqual([(s.name, s.unit) for s in chosen],
                                 [("alpha", "break-alpha"), ("beta", "beta"), ("alpha", "break-alpha")])
                with self.assertRaises(SystemExit):
                    qa_parity.chosen_scenarios(["gamma"])


class TwinlessScenarioTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        scenarios, mutations = self.root / "scenarios", self.root / "mutations"
        (scenarios / "test_example").mkdir(parents=True)
        (scenarios / "hidden_files").mkdir()
        mutations.mkdir()
        (scenarios / "test_example" / "alpha.jsonl").write_text(
            json.dumps({"scenario": "alpha", "modes": ["columns"], "twin": "x", "mutation": "break-alpha"}) + "\n")
        (scenarios / "test_example" / "beta.jsonl").write_text(
            json.dumps({"scenario": "beta", "modes": ["columns"], "twin": "y"}) + "\n")
        (scenarios / "hidden_files" / "gamma.jsonl").write_text(
            json.dumps({"scenario": "gamma", "modes": ["columns"], "skip": "flaky"}) + "\n")
        (scenarios / "hidden_files" / "delta.jsonl").write_text(
            json.dumps({"scenario": "delta", "modes": ["columns", "list"]}) + "\n")
        (mutations / "break-alpha.patch").write_text("+++ b/src/main.rs\n")
        for name, value in (("SCENARIOS", scenarios), ("MUTATIONS", mutations),
                            ("OUTPUT", self.root / "output"), ("CI_SHARDS", 1)):
            patcher = mock.patch.object(qa_parity, name, value)
            patcher.start()
            self.addCleanup(patcher.stop)

    def test_a_scenario_without_a_twin_takes_no_part_and_is_reported_as_no_twin(self):
        self.assertEqual([s.name for s in qa_parity.chosen_scenarios([])], ["alpha", "beta"])
        self.assertEqual(qa_parity.chosen_scenarios(["delta", "gamma"]), [])
        self.assertEqual(qa_parity.ci_units(), ["beta", "break-alpha"])
        self.assertEqual(
            qa_parity.skipped_lines(["beta", "delta", "gamma"]),
            ["No twin, not compared: delta", "No twin, not compared: gamma"],
        )
        self.assertEqual(qa_parity.skipped_lines([]),
                         ["No twin, not compared: delta", "No twin, not compared: gamma"])

    def test_naming_only_scenarios_without_a_twin_runs_nothing(self):
        with mock.patch.object(qa_parity, "collect") as collect, \
                contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(qa_parity.parity(["delta"]), 0)
        collect.assert_not_called()
        summary = (self.root / "output" / "summary.md").read_text()
        self.assertIn("0 of 0 pairs agree", summary)
        self.assertIn("No twin, not compared: delta", summary)

    def test_a_qa_log_line_without_a_twin_pairs_with_nothing(self):
        log = "\n".join([
            "RUN delta/columns",
            f"RUN beta/columns (twin {TWIN}[columns])",
            "PASS delta/columns",
            "delta/list failed:",
            f"PASS beta/columns (twin {TWIN}[columns])",
        ])
        self.assertEqual(
            {twin: (run.scenario, run.result.status) for twin, run in parse_qa_log(log).items()},
            {f"{TWIN}[columns]": ("beta", "pass")},
        )


class AggregateTests(unittest.TestCase):
    STATE = {"view_mode": "Columns", "directory": "d", "panes": [], "focused": None,
             "focused_pane": None, "dialog": None}

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        root = Path(self.directory.name)
        scenarios, mutations = root / "scenarios", root / "mutations"
        (scenarios / "test_example").mkdir(parents=True)
        mutations.mkdir()
        for name in ("alpha", "beta"):
            (scenarios / "test_example" / f"{name}.jsonl").write_text(json.dumps({"scenario": name, "twin": "x", "mutation": f"break-{name}"}) + "\n")
            (mutations / f"break-{name}.patch").write_text("+++ b/src/main.rs\n")
        (scenarios / "test_example" / "gamma.jsonl").write_text(json.dumps({"scenario": "gamma", "twin": "x", "mutation": None}) + "\n")
        self.shards = root / "shards"
        for name, value in (("SCENARIOS", scenarios), ("MUTATIONS", mutations),
                            ("OUTPUT", root / "untouched"), ("CI_SHARDS", 3)):
            patcher = mock.patch.object(qa_parity, name, value)
            patcher.start()
            self.addCleanup(patcher.stop)
        patcher = mock.patch.dict(os.environ)
        patcher.start()
        self.addCleanup(patcher.stop)
        os.environ.pop("GITHUB_STEP_SUMMARY", None)
        quiet = contextlib.redirect_stdout(io.StringIO())
        quiet.__enter__()
        self.addCleanup(quiet.__exit__, None, None, None)

    def write_shard(self, index, scenario, attempt=1, mutated=True):
        tree = self.shards / f"qa-parity-{attempt}-shard-{index}"
        twin = f"tests/e2e/scenarios/test_{scenario}.py::test_{scenario}[columns]"
        stem = qa_parity.node_file_name(twin)

        def side(destination, failed):
            run = destination / "qa" / scenario / "columns"
            run.mkdir(parents=True)
            outcome = f"{scenario}/columns (twin {twin}) failed:" if failed else f"PASS {scenario}/columns (twin {twin})"
            (destination / "qa.log").write_text(f"RUN {scenario}/columns (twin {twin})\n{outcome}\n")
            (destination / "qa.status").write_text("101\n" if failed else "0\n")
            failures = [{"line": 3, "step": {"wait": {}, "message": "the copy"}, "text": ""}] if failed else []
            (run / "result.json").write_text(json.dumps({"twin": twin, "failures": failures}))
            failure = ('<failure message="TreeTimeout: timed out after 15s waiting for the copy"/>'
                       if failed else "")
            (destination / "python").mkdir()
            (destination / "python" / "junit.xml").write_text(
                f'<testsuite><testcase classname="tests.e2e.scenarios.test_{scenario}" '
                f'name="test_{scenario}[columns]">{failure}</testcase></testsuite>'
            )
            return run

        run = side(tree / "baseline", failed=False)
        (run / "input.log").write_text("key 36 down\nkey 36 up\n")
        (run / "state.json").write_text(json.dumps(self.STATE))
        for kind, text in (("input", "key 36 down\nkey 36 up\n"), ("state", json.dumps({"app": self.STATE}))):
            (tree / "baseline" / "python" / kind).mkdir()
            (tree / "baseline" / "python" / kind / f"{stem}.{'log' if kind == 'input' else 'json'}").write_text(text)
        if mutated:
            side(tree / "mutations" / f"break-{scenario}", failed=True)
        (tree / "summary.md").write_text("...\n\n1 of 1 pairs agree (1.5 minutes).\n")

    def test_one_verdict_requires_every_planned_shard_to_agree(self):
        self.write_shard(0, "alpha")
        self.write_shard(1, "beta")
        self.assertEqual(qa_parity.aggregate(self.shards), 1)
        summary = (self.shards / "summary.md").read_text()
        self.assertIn("2 of 3 pairs agree", summary)
        self.assertIn("shard 2 uploaded no parity report", summary)
        self.assertIn("| 0 | break-alpha | 1.5 | agree |", summary)

        self.write_shard(2, "gamma", mutated=False)
        self.assertEqual(qa_parity.aggregate(self.shards), 0)
        self.assertFalse((self.shards.parent / "untouched").exists())
        summary = (self.shards / "summary.md").read_text()
        self.assertIn("3 of 3 pairs agree", summary)
        self.assertIn("| gamma | – | 1 | agree | n/a | n/a | agree | agree |", summary)

        (self.shards / "qa-parity-1-shard-3").mkdir()
        self.assertEqual(qa_parity.aggregate(self.shards), 1)
        self.assertIn("not in this checkout's shard plan", (self.shards / "summary.md").read_text())

    def test_a_shard_that_did_not_finish_fails_even_when_its_reports_agree(self):
        self.write_shard(0, "alpha")
        self.write_shard(1, "beta")
        self.write_shard(2, "gamma", mutated=False)
        summary = self.shards / "qa-parity-1-shard-1" / "summary.md"
        summary.write_text("Parity did not finish: could not restore src/main.rs\n")
        self.assertEqual(qa_parity.aggregate(self.shards), 1)
        self.assertIn("3 of 3 pairs agree", (self.shards / "summary.md").read_text())
        self.assertIn("DISAGREE: Parity did not finish", (self.shards / "summary.md").read_text())

    def test_each_shard_counts_only_its_newest_attempt(self):
        self.write_shard(0, "alpha")
        self.write_shard(2, "gamma", mutated=False)
        self.write_shard(1, "beta", attempt=1)
        (self.shards / "qa-parity-1-shard-1" / "summary.md").write_text("Parity did not finish: runner lost\n")
        self.write_shard(1, "beta", attempt=2)
        self.assertEqual(qa_parity.aggregate(self.shards), 0)

        (self.shards / "qa-parity-3-shard-1").mkdir()
        self.assertEqual(qa_parity.aggregate(self.shards), 1)
        self.assertIn("shard 1 uploaded no parity report", (self.shards / "summary.md").read_text())


class CiWorkflowTests(unittest.TestCase):
    def jobs(self):
        workflow = (REPOSITORY / ".github/workflows/ci.yml").read_text()
        return dict(re.findall(r"^  ([\w-]+):\n(.*?)(?=^  [\w-]+:\n|\Z)",
                               workflow.split("\njobs:\n", 1)[1], re.M | re.S))

    def test_qa_jobs_are_informational_and_outside_the_required_gates(self):
        jobs = self.jobs()
        informational = ("qa", "qa-parity-plan", "qa-parity-shard", "qa-parity")
        for name in informational:
            with self.subTest(job=name):
                self.assertRegex(jobs[name], r"(?m)^    continue-on-error: true$")
        needs = re.search(r"^    needs: (.+)$", jobs["quality"], re.M).group(1)
        self.assertFalse(set(re.findall(r"[\w-]+", needs)) & set(informational))

    def test_both_release_marker_checks_grep_a_prefix_of_the_qa_usage_constant(self):
        source = (REPOSITORY / "src/qa.rs").read_text()
        usage = re.search(r'^const USAGE: &str = "([^"]+)";$', source, re.M).group(1)
        self.assertIn("return Err(USAGE.to_owned());", source)
        release = (REPOSITORY / ".github/workflows/release.yml").read_text()
        check = release.split("- name: Check the release binary has no QA mode", 1)[1].split("\n      - name:", 1)[0]
        pattern = re.search(r'if strings "target/\$TARGET/release/strata" \| grep -F "([^"]+)"; then', check).group(1)
        self.assertTrue(usage.startswith(pattern), (usage, pattern))
        self.assertIn(f'| grep -F "{pattern}"', self.jobs()["qa"])


if __name__ == "__main__":
    unittest.main()
