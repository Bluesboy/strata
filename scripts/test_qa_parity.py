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
    parse_junit,
    parse_qa_log,
    pass_parity,
    qa_signature,
    shard_units,
    signature_parity,
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


class CiShardingTests(unittest.TestCase):
    def test_every_scenario_lands_in_exactly_one_shard_by_its_patch_or_name(self):
        units = qa_parity.ci_units()
        found = qa_parity.scenarios()
        self.assertEqual(sorted(units), sorted(scenario.mutation or scenario.name for scenario in found))
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
            scenarios.mkdir()
            mutations.mkdir()
            (scenarios / "alpha.jsonl").write_text(json.dumps({"scenario": "alpha", "mutation": "break-alpha"}) + "\n")
            (scenarios / "beta.jsonl").write_text(json.dumps({"scenario": "beta", "mutation": None}) + "\n")
            (mutations / "break-alpha.patch").write_text("+++ b/src/main.rs\n")
            with mock.patch.object(qa_parity, "SCENARIOS", scenarios), \
                    mock.patch.object(qa_parity, "MUTATIONS", mutations), \
                    mock.patch.object(qa_parity, "CI_SHARDS", 1):
                self.assertEqual(qa_parity.ci_units(), ["beta", "break-alpha"])
                output = io.StringIO()
                with contextlib.redirect_stdout(output):
                    self.assertEqual(qa_parity.ci_plan(None), 0)
                self.assertIn('"names":"beta break-alpha"', output.getvalue())
                (scenarios / "break-alpha.jsonl").write_text(json.dumps({"scenario": "break-alpha", "mutation": None}) + "\n")
                with self.assertRaises(SystemExit):
                    qa_parity.ci_units()

    def test_pull_requests_run_parity_only_when_either_twin_can_change(self):
        cases = [
            (["src/ui/browser/mod.rs"], True),
            (["tests/e2e/scenarios/test_clipboard.py"], True),
            (["tests/qa/scenarios/copy-leaves-source.jsonl"], True),
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
            (scenarios / "alpha.jsonl").write_text(json.dumps({"scenario": "alpha", "mutation": "break-alpha"}) + "\n")
            (scenarios / "beta.jsonl").write_text(json.dumps({"scenario": "beta", "mutation": None}) + "\n")
            with mock.patch.object(qa_parity, "SCENARIOS", scenarios):
                chosen = qa_parity.chosen_scenarios(["break-alpha", "beta", "alpha"])
                self.assertEqual([(s.name, s.unit) for s in chosen],
                                 [("alpha", "break-alpha"), ("beta", "beta"), ("alpha", "break-alpha")])
                with self.assertRaises(SystemExit):
                    qa_parity.chosen_scenarios(["gamma"])


class AggregateTests(unittest.TestCase):
    STATE = {"view_mode": "Columns", "directory": "d", "panes": [], "focused": None,
             "focused_pane": None, "dialog": None}

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        root = Path(self.directory.name)
        scenarios, mutations = root / "scenarios", root / "mutations"
        scenarios.mkdir()
        mutations.mkdir()
        for name in ("alpha", "beta"):
            (scenarios / f"{name}.jsonl").write_text(json.dumps({"scenario": name, "mutation": f"break-{name}"}) + "\n")
            (mutations / f"break-{name}.patch").write_text("+++ b/src/main.rs\n")
        (scenarios / "gamma.jsonl").write_text(json.dumps({"scenario": "gamma", "mutation": None}) + "\n")
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
        for gate in ("quality", "e2e"):
            needs = re.search(r"^    needs: (.+)$", jobs[gate], re.M).group(1)
            self.assertFalse(set(re.findall(r"[\w-]+", needs)) & set(informational), gate)

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
