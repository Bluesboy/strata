# SPDX-License-Identifier: MIT

import tempfile
import unittest
from pathlib import Path

from qa_parity import (
    QaRun,
    Result,
    RunPair,
    compare_input,
    compare_state,
    exit_accounted,
    fail_parity,
    parse_junit,
    parse_qa_log,
    pass_parity,
    qa_signature,
    signature_parity,
)

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
                "</testsuite></testsuites>"
            )
            results = parse_junit(report)
        self.assertEqual(results[f"{TWIN}[columns]"].signature, "the copy to land in archive")
        self.assertEqual(results[f"{TWIN}[icons]"].signature, "cut must not touch the filesystem")
        self.assertEqual(results[f"{TWIN}[list]"].status, "error")

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


if __name__ == "__main__":
    unittest.main()
