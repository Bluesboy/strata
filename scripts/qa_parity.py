#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Proves the `--qa` scenarios and their Python twins agree.

For every scenario in tests/qa/scenarios (or the ones whose `mutation` is
named), inside the pinned container runners and one after the other:

- pass parity: both sides pass on the unmodified tree, run by run;
- fail parity: under the scenario's mutation patch, each run fails on both
  sides or on neither, and at least one run fails on both;
- failure-signature parity: where both fail, the Python wait message equals
  the `message` of the QA step that failed;
- input parity: the key and pointer-button lines of the two input logs agree
  in order, pointer positions within one pixel;
- final-state parity: the compared keys of the final state snapshots agree.

QA runs map to pytest node ids through the `(twin ...)` on their PASS and
failed lines. Reports go to target/qa-parity: one `<patch>.md` per patch and
`summary.md`. See "QA parity" in docs/e2e-testing.md.

`scripts/e2e-mutation-check.sh --qa` reuses the `scenario`, `twins`, and
`verdict` subcommands for pass and fail parity alone.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import time
import xml.etree.ElementTree as ET
from dataclasses import dataclass, field
from pathlib import Path

REPOSITORY = Path(__file__).resolve().parents[1]
SCENARIOS = REPOSITORY / "tests/qa/scenarios"
MUTATIONS = REPOSITORY / "tests/e2e/mutations"
OUTPUT = REPOSITORY / "target/qa-parity"
QA_ARTIFACTS = REPOSITORY / "target/qa-artifacts"
PIXEL_TOLERANCE = 1
# "Replies and the state snapshot" in docs/e2e-testing.md.
COMPARED_STATE = ("view_mode", "directory", "panes", "focused", "focused_pane", "dialog")
# Both runners then name each fixture directory after the twin's node id.
PARITY_FIXTURES = {"STRATA_PARITY_FIXTURES": "1"}
# Same file naming as `node_file_name` in tests/e2e/conftest.py.
UNSAFE_NAME = re.compile(r"[^A-Za-z0-9._-]+")

RUN = r"(?P<scenario>[^/\s]+)/(?P<run>\S+) \(twin (?P<twin>.+?)\)"
QA_PLANNED = re.compile(rf"^RUN {RUN}$")
QA_PASS = re.compile(rf"^PASS {RUN}$")
QA_KNOWN_BUG = re.compile(rf"^KNOWN BUG {RUN} still fails as expected")
QA_FAILED = re.compile(
    rf"^{RUN} (?P<outcome>failed:|could not run:|passes but is marked known_bug|is marked known_bug)"
)
WAITING_FOR = re.compile(r"timed out after \d+s waiting for (?P<message>.+)")
EXCEPTION_PREFIX = re.compile(r"^(?:[A-Za-z_][\w.]*(?:Error|Timeout|Exception)): ")
INPUT_KEY = re.compile(r"^key (\d+) (down|up)$")
INPUT_POINTER = re.compile(r"^pointer (-?\d+) (-?\d+) button (\d+) (down|up)$")
INPUT_MOTION = re.compile(r"^motion -?\d+ -?\d+$")


# ------------------------------------------------------------------ scenarios

@dataclass(frozen=True)
class Scenario:
    name: str
    mutation: str | None


def scenarios() -> list[Scenario]:
    found = []
    for path in sorted(SCENARIOS.glob("*.jsonl")):
        header = json.loads(path.read_text(encoding="utf-8").splitlines()[0])
        found.append(Scenario(path.stem, header.get("mutation")))
    return found


def scenario_for(patch: str) -> Scenario:
    matching = [scenario for scenario in scenarios() if scenario.mutation == patch]
    if len(matching) != 1:
        raise SystemExit(f"{patch}: expected one QA scenario naming it as its mutation, found {len(matching)}")
    return matching[0]


def node_file_name(node_id: str) -> str:
    return UNSAFE_NAME.sub("_", node_id)


# -------------------------------------------------------------------- results

@dataclass
class Result:
    """One run on one side: `pass`, `fail`, `error`, `known bug`, or `missing`."""

    status: str
    signature: str = ""
    detail: str = ""


@dataclass
class QaRun:
    scenario: str
    run: str
    twin: str
    result: Result


def parse_qa_log(text: str) -> dict[str, QaRun]:
    """QA runs by twin node id.

    `tests/qa.rs` prints a `RUN` line for every planned run before any runs,
    then a PASS, KNOWN BUG, or failed line for each. A planned run without a
    result line stays `missing`, so a truncated run can never drop out.
    Parity does not accept known bugs: a twin pair must agree outright.
    """

    runs: dict[str, QaRun] = {}
    for line in text.splitlines():
        line = line.rstrip()
        if match := QA_PLANNED.match(line):
            twin = match["twin"]
            if twin not in runs:
                runs[twin] = QaRun(match["scenario"], match["run"], twin,
                                   Result("missing", detail="planned, but no result line"))
            continue
        if match := QA_PASS.match(line):
            status = "pass"
        elif match := QA_KNOWN_BUG.match(line):
            status = "known bug"
        elif match := QA_FAILED.match(line):
            status = "fail" if match["outcome"] == "failed:" else "error"
        else:
            continue
        twin = match["twin"]
        runs[twin] = QaRun(match["scenario"], match["run"], twin, Result(status))
    return runs


def qa_signature(result: dict) -> tuple[str, str]:
    """The failed step's twin message, and where it is, from a run's result.json."""

    failures = result.get("failures") or []
    if not failures:
        return "", ""
    failure = failures[0]
    step = failure.get("step") or {}
    line = failure.get("line")
    message = step.get("message") if isinstance(step, dict) else None
    if not message:
        message = failure.get("text", "")
    return normalise(message), f"line {line}: {json.dumps(step, separators=(',', ':'))}"


def add_qa_details(runs: dict[str, QaRun], artifacts: Path) -> None:
    for run in runs.values():
        if run.result.status not in ("pass", "fail", "error"):
            continue
        path = artifacts / run.scenario / run.run / "result.json"
        if not path.is_file():
            run.result = Result("error", detail=f"{run.result.status}, but it wrote no result.json")
            continue
        result = json.loads(path.read_text(encoding="utf-8"))
        if result.get("twin") != run.twin:
            run.result = Result("error", detail=f"result.json names twin {result.get('twin')}")
            continue
        if run.result.status == "fail":
            run.result.signature, run.result.detail = qa_signature(result)
        elif run.result.status == "error":
            run.result.detail = "; ".join(result.get("errors") or [])


def parse_junit(report: Path) -> dict[str, Result]:
    """Python results by node id."""

    try:
        root = ET.parse(report).getroot()
    except (ET.ParseError, OSError):
        return {}
    results: dict[str, Result] = {}
    for case in root.iter("testcase"):
        classname = case.get("classname", "")
        node = classname.replace(".", "/") + ".py::" + case.get("name", "")
        failure = case.find("failure")
        error = case.find("error")
        if error is not None:
            results[node] = Result("error", detail=first_line(error.get("message", "")))
        elif failure is not None:
            message = failure.get("message", "")
            results[node] = Result("fail", python_signature(message), first_line(message))
        elif case.find("skipped") is not None:
            results[node] = Result("error", detail="skipped")
        else:
            results[node] = Result("pass")
    return results


def python_signature(message: str) -> str:
    """A `strata.wait` message, or else an assertion's message or expression."""

    if match := WAITING_FOR.search(message):
        return normalise(match["message"])
    return normalise(EXCEPTION_PREFIX.sub("", first_line(message)))


def first_line(text: str) -> str:
    return text.strip().splitlines()[0] if text.strip() else ""


def normalise(text: str) -> str:
    return " ".join(text.split())


# ---------------------------------------------------------------- comparisons

@dataclass
class Comparison:
    agree: bool
    note: str = ""


def compare_input(python: str, qa: str) -> Comparison:
    """Key and pointer-button lines in order; pointer positions within a pixel."""

    def events(text: str, side: str) -> list[tuple]:
        found = []
        for line in text.splitlines():
            line = line.strip()
            if match := INPUT_KEY.match(line):
                found.append(("key", int(match[1]), match[2]))
            elif match := INPUT_POINTER.match(line):
                found.append(("pointer", int(match[1]), int(match[2]), int(match[3]), match[4]))
            elif line and not INPUT_MOTION.match(line):
                raise ValueError(f"{side} logged an unrecognised line: {line!r}")
        return found

    try:
        left, right = events(python, "Python"), events(qa, "QA")
    except ValueError as error:
        return Comparison(False, str(error))
    if not left and not right:
        return Comparison(False, "neither side logged input")
    for index, (one, other) in enumerate(zip(left, right)):
        if one[0] != other[0]:
            return Comparison(False, f"event {index + 1}: Python {one}, QA {other}")
        if one[0] == "key" and one != other:
            return Comparison(False, f"event {index + 1}: Python {one}, QA {other}")
        if one[0] == "pointer" and (
            one[3:] != other[3:]
            or abs(one[1] - other[1]) > PIXEL_TOLERANCE
            or abs(one[2] - other[2]) > PIXEL_TOLERANCE
        ):
            return Comparison(False, f"event {index + 1}: Python {one}, QA {other}")
    if len(left) != len(right):
        return Comparison(False, f"Python sent {len(left)} events, QA {len(right)}")
    return Comparison(True, f"{len(left)} events")


def compare_state(python: dict | None, qa: dict | None) -> Comparison:
    if not python or "error" in python:
        return Comparison(False, f"no Python state: {(python or {}).get('error', 'missing')}")
    if not qa:
        return Comparison(False, "no QA state")
    for side, state in (("Python", python), ("QA", qa)):
        absent = [key for key in COMPARED_STATE if key not in state]
        if absent:
            return Comparison(False, f"the {side} state lacks {', '.join(absent)}")
    differing = [key for key in COMPARED_STATE if python.get(key) != qa.get(key)]
    if differing:
        return Comparison(
            False,
            "; ".join(f"{key}: Python {python.get(key)!r}, QA {qa.get(key)!r}" for key in differing),
        )
    return Comparison(True)


# ------------------------------------------------------------------- verdicts

@dataclass
class RunPair:
    twin: str
    label: str
    qa: Result
    python: Result


def pairs(qa_runs: dict[str, QaRun], python: dict[str, Result]) -> list[RunPair]:
    found = [
        RunPair(twin, f"{run.scenario}/{run.run}", run.result, python.get(twin, Result("missing")))
        for twin, run in qa_runs.items()
    ]
    found.extend(
        RunPair(twin, "(no QA run)", Result("missing"), result)
        for twin, result in python.items()
        if twin not in qa_runs
    )
    return found


def pass_parity(found: list[RunPair]) -> Comparison:
    bad = [pair for pair in found if pair.qa.status != "pass" or pair.python.status != "pass"]
    if not found:
        return Comparison(False, "no runs")
    if bad:
        return Comparison(False, "; ".join(
            f"{pair.label}: QA {pair.qa.status}, Python {pair.python.status}" for pair in bad
        ))
    return Comparison(True, f"{len(found)} runs")


def fail_parity(found: list[RunPair]) -> Comparison:
    """Each run fails on both sides or neither; infrastructure errors never count."""

    if not found:
        return Comparison(False, "no runs")
    problems = []
    for pair in found:
        statuses = {pair.qa.status, pair.python.status}
        if not statuses <= {"pass", "fail"}:
            problems.append(f"{pair.label}: QA {pair.qa.status}, Python {pair.python.status}")
        elif len(statuses) == 2:
            problems.append(f"{pair.label}: only {'QA' if pair.qa.status == 'fail' else 'Python'} failed")
    if problems:
        return Comparison(False, "; ".join(problems))
    failing = sum(pair.qa.status == "fail" for pair in found)
    if not failing:
        return Comparison(False, "the mutation went undetected on both sides")
    return Comparison(True, f"{failing} of {len(found)} runs fail on both sides")


def signature_parity(found: list[RunPair]) -> Comparison:
    both = [pair for pair in found if pair.qa.status == pair.python.status == "fail"]
    mismatched = [
        pair for pair in both
        if not pair.qa.signature or not pair.python.signature
        or pair.qa.signature != pair.python.signature
    ]
    if not both:
        return Comparison(False, "no run failed on both sides")
    if mismatched:
        return Comparison(False, "; ".join(
            f"{pair.label}: Python {pair.python.signature!r}, QA {pair.qa.signature!r}"
            for pair in mismatched
        ))
    return Comparison(True, f"{len(both)} runs")


# ---------------------------------------------------------------- the runners

def run_logged(command: list[str], log: Path, environment: dict[str, str]) -> int:
    log.parent.mkdir(parents=True, exist_ok=True)
    started = time.monotonic()
    with log.open("w", encoding="utf-8") as output:
        status = subprocess.run(
            command, cwd=REPOSITORY, stdout=output, stderr=subprocess.STDOUT,
            env={**os.environ, **environment}, check=False,
        ).returncode
    print(f"   {' '.join(command[:1] + command[1:3])}... exited {status} after "
          f"{time.monotonic() - started:.0f}s; log {log.relative_to(REPOSITORY)}", flush=True)
    return status


def run_qa(names: list[str], destination: Path) -> None:
    for name in names:
        shutil.rmtree(QA_ARTIFACTS / name, ignore_errors=True)
    status = run_logged(
        [str(REPOSITORY / "scripts/qa.sh"), *names],
        destination / "qa.log",
        {"STRATA_QA_KEEP_ARTIFACTS": "1", **PARITY_FIXTURES},
    )
    (destination / "qa.status").write_text(f"{status}\n")
    artifacts = destination / "qa"
    shutil.rmtree(artifacts, ignore_errors=True)
    for name in names:
        if (QA_ARTIFACTS / name).is_dir():
            shutil.copytree(QA_ARTIFACTS / name, artifacts / name)


def load_qa(destination: Path) -> dict[str, QaRun]:
    log = destination / "qa.log"
    if not log.is_file():
        return {}
    runs = parse_qa_log(log.read_text(encoding="utf-8", errors="replace"))
    add_qa_details(runs, destination / "qa")
    return runs


def load_qa_status(destination: Path) -> int | None:
    try:
        return int((destination / "qa.status").read_text())
    except (OSError, ValueError):
        return None


def exit_accounted(status: int | None, runs) -> Comparison:
    """qa.sh fails exactly when a run did; anything else is unaccounted for."""

    if status is None:
        return Comparison(False, "qa.sh's exit status was not recorded")
    failing = any(run.result.status != "pass" for run in runs)
    if (status != 0) != failing:
        return Comparison(False, f"qa.sh exited {status}, which its run results do not account for")
    return Comparison(True)


def run_python(twins: list[str], destination: Path) -> None:
    python = destination / "python"
    shutil.rmtree(python, ignore_errors=True)
    python.mkdir(parents=True)
    report = python / "junit.xml"
    run_logged(
        [
            str(REPOSITORY / "scripts/e2e.sh"), "-q", "--maxfail=0",
            f"--junitxml={report.relative_to(REPOSITORY)}", *twins,
        ],
        destination / "python.log",
        {
            "STRATA_INPUT_LOG": str(python / "input"),
            "STRATA_STATE_DUMP": str(python / "state"),
            **PARITY_FIXTURES,
        },
    )


def load_python(destination: Path) -> dict[str, Result]:
    return parse_junit(destination / "python" / "junit.xml")


def patched_files(patch: Path) -> list[str]:
    return sorted({
        line[len("+++ b/"):].strip()
        for line in patch.read_text(encoding="utf-8").splitlines()
        if line.startswith("+++ b/")
    })


def git(*arguments: str) -> subprocess.CompletedProcess:
    return subprocess.run(
        ["git", "-C", str(REPOSITORY), *arguments], capture_output=True, text=True, check=False
    )


class Mutation:
    """Applies a patch and restores exactly the files it touched."""

    def __init__(self, name: str) -> None:
        self.patch = MUTATIONS / f"{name}.patch"
        if not self.patch.is_file():
            raise SystemExit(f"unknown mutation: {name}")
        self.files = patched_files(self.patch)
        if not self.files:
            raise SystemExit(f"{self.patch} names no files")

    def __enter__(self) -> Mutation:
        dirty = git("status", "--porcelain", "--", *self.files).stdout.strip()
        if dirty:
            raise SystemExit(f"{self.patch.name} touches files with uncommitted changes:\n{dirty}")
        try:
            applied = git("apply", "-p1", str(self.patch))
            if applied.returncode:
                raise SystemExit(f"{self.patch.name} does not apply:\n{applied.stderr}")
        except BaseException:
            # The files were clean, so restoring them undoes any partial apply.
            self.__exit__(None, None, None)
            raise
        return self

    def __exit__(self, *_exception) -> None:
        git("restore", "--worktree", "--", *self.files)
        left = git("status", "--porcelain", "--", *self.files).stdout.strip()
        if left:
            raise SystemExit(f"could not restore {self.files} after {self.patch.name}:\n{left}")


# --------------------------------------------------------------------- report

@dataclass
class PairReport:
    scenario: str
    patch: str
    results: dict[str, Comparison] = field(default_factory=dict)
    baseline: list[RunPair] = field(default_factory=list)
    mutated: list[RunPair] = field(default_factory=list)
    inputs: dict[str, Comparison] = field(default_factory=dict)
    states: dict[str, Comparison] = field(default_factory=dict)

    @property
    def agree(self) -> bool:
        return all(comparison.agree for comparison in self.results.values())


def compare_baseline(report: PairReport, destination: Path) -> None:
    artifacts = destination / "qa"
    python = destination / "python"
    for pair in report.baseline:
        if pair.label == "(no QA run)":
            continue
        scenario, run = pair.label.split("/", 1)
        stem = node_file_name(pair.twin)
        python_input = python / "input" / f"{stem}.log"
        qa_input = artifacts / scenario / run / "input.log"
        report.inputs[pair.label] = (
            compare_input(python_input.read_text(), qa_input.read_text())
            if python_input.is_file() and qa_input.is_file()
            else Comparison(False, "an input log is missing")
        )
        python_state = python / "state" / f"{stem}.json"
        qa_state = artifacts / scenario / run / "state.json"
        report.states[pair.label] = compare_state(
            json.loads(python_state.read_text())["app"] if python_state.is_file() else None,
            json.loads(qa_state.read_text()) if qa_state.is_file() else None,
        )

    def combined(comparisons: dict[str, Comparison]) -> Comparison:
        bad = [f"{label}: {item.note}" for label, item in comparisons.items() if not item.agree]
        if not comparisons:
            return Comparison(False, "nothing compared")
        return Comparison(not bad, "; ".join(bad) or f"{len(comparisons)} runs")

    report.results["input"] = combined(report.inputs)
    report.results["state"] = combined(report.states)


def mark(comparison: Comparison | None) -> str:
    if comparison is None:
        return "not run"
    return "agree" if comparison.agree else "DISAGREE"


def write_patch_report(report: PairReport, output: Path = OUTPUT) -> Path:
    def cell(text: str) -> str:
        return text.replace("|", "\\|").replace("\n", " ") or "–"

    lines = [
        f"# {report.patch}",
        "",
        f"QA scenario `{report.scenario}`; patch `tests/e2e/mutations/{report.patch}.patch`.",
        "",
        "| Check | Result | Notes |",
        "| --- | --- | --- |",
    ]
    for check in ("pass", "fail", "signature", "input", "state"):
        comparison = report.results.get(check)
        lines.append(f"| {check} | {mark(comparison)} | {cell(comparison.note if comparison else '')} |")
    lines += [
        "",
        "## Failure signatures under the patch",
        "",
        "| Run | Twin | Python | Python wait message | QA | QA step message | QA step |",
        "| --- | --- | --- | --- | --- | --- | --- |",
    ]
    for pair in report.mutated:
        lines.append(
            f"| {cell(pair.label)} | `{cell(pair.twin)}` | {pair.python.status} | "
            f"{cell(pair.python.signature or pair.python.detail)} | {pair.qa.status} | "
            f"{cell(pair.qa.signature)} | {cell(pair.qa.detail)} |"
        )
    lines += [
        "",
        "## Unmodified runs",
        "",
        "| Run | Python | QA | Input | State |",
        "| --- | --- | --- | --- | --- |",
    ]

    def described(comparison: Comparison | None) -> str:
        if comparison is None:
            return "not compared"
        return mark(comparison) + (f": {comparison.note}" if comparison.note else "")

    for pair in report.baseline:
        lines.append(
            f"| {cell(pair.label)} | {pair.python.status} | {pair.qa.status} | "
            f"{cell(described(report.inputs.get(pair.label)))} | "
            f"{cell(described(report.states.get(pair.label)))} |"
        )
    path = output / f"{report.patch}.md"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return path


def write_summary(reports: list[PairReport], elapsed: float | None, extra: list[str] = (),
                  output: Path | None = None) -> str:
    header = "| Scenario | Patch | Runs | Pass | Fail | Signature | Input | State |"
    lines = [header, "| --- | --- | --- | --- | --- | --- | --- | --- |"]
    for report in reports:
        lines.append(
            f"| {report.scenario} | {report.patch} | {len(report.baseline)} | "
            + " | ".join(mark(report.results.get(check)) for check in ("pass", "fail", "signature", "input", "state"))
            + " |"
        )
    agreeing = sum(report.agree for report in reports)
    took = f" ({elapsed / 60:.1f} minutes)" if elapsed is not None else ""
    lines += ["", f"{agreeing} of {len(reports)} pairs agree{took}.", *extra]
    text = "\n".join(lines) + "\n"
    output = output or OUTPUT
    output.mkdir(parents=True, exist_ok=True)
    (output / "summary.md").write_text(text, encoding="utf-8")
    return text


# ----------------------------------------------------------------------- main

def chosen_scenarios(patches: list[str]) -> list[Scenario]:
    if patches:
        return [scenario_for(patch) for patch in patches]
    return scenarios()


def baseline_pairs(scenario: Scenario, qa: dict[str, QaRun], python: dict[str, Result]) -> list[RunPair]:
    own = {twin: run for twin, run in qa.items() if run.scenario == scenario.name}
    return pairs(own, {twin: python.get(twin, Result("missing")) for twin in own})


def collect(chosen: list[Scenario]) -> None:
    """Runs both sides, unmodified and under each patch, into target/qa-parity."""

    baseline = OUTPUT / "baseline"
    shutil.rmtree(baseline, ignore_errors=True)
    print("== unmodified tree: QA scenarios", flush=True)
    run_qa([scenario.name for scenario in chosen], baseline)
    qa = load_qa(baseline)
    print("== unmodified tree: Python twins", flush=True)
    run_python(sorted(qa), baseline)
    python = load_python(baseline)
    for scenario in chosen:
        if not scenario.mutation:
            continue
        destination = OUTPUT / "mutations" / scenario.mutation
        shutil.rmtree(destination, ignore_errors=True)
        found = baseline_pairs(scenario, qa, python)
        if not pass_parity(found).agree:
            print(f"== {scenario.mutation}: skipped, the unmodified pair does not pass", flush=True)
            continue
        print(f"== {scenario.mutation}: mutated tree", flush=True)
        with Mutation(scenario.mutation):
            run_qa([scenario.name], destination)
            run_python([pair.twin for pair in found], destination)


def evaluate(chosen: list[Scenario], output: Path = OUTPUT) -> list[PairReport]:
    """Compares what `collect` saved under `output`; writes `<patch>.md` there for each pair."""

    baseline = output / "baseline"
    qa, python = load_qa(baseline), load_python(baseline)
    baseline_exit = exit_accounted(load_qa_status(baseline), qa.values())
    reports = []
    for scenario in chosen:
        report = PairReport(scenario.name, scenario.mutation or "(none)")
        report.baseline = baseline_pairs(scenario, qa, python)
        report.results["pass"] = pass_parity(report.baseline)
        if report.results["pass"].agree and not baseline_exit.agree:
            report.results["pass"] = baseline_exit
        compare_baseline(report, baseline)
        destination = output / "mutations" / report.patch
        if not scenario.mutation:
            report.results["fail"] = Comparison(False, "the scenario names no mutation")
        elif not (destination / "qa.log").is_file():
            report.results["fail"] = Comparison(False, "not run: the unmodified pair does not pass")
        else:
            twins = [pair.twin for pair in report.baseline]
            mutated = load_python(destination)
            mutated_qa = load_qa(destination)
            report.mutated = pairs(
                mutated_qa, {twin: mutated.get(twin, Result("missing")) for twin in twins}
            )
            report.results["fail"] = fail_parity(report.mutated)
            exited = exit_accounted(load_qa_status(destination), mutated_qa.values())
            if report.results["fail"].agree and not exited.agree:
                report.results["fail"] = exited
            report.results["signature"] = signature_parity(report.mutated)
        report.results.setdefault("signature", Comparison(False, "not run"))
        if scenario.mutation:
            write_patch_report(report, output)
        reports.append(report)
    return reports


def parity(patches: list[str], rerun: bool = True) -> int:
    started = time.monotonic()
    chosen = chosen_scenarios(patches)
    OUTPUT.mkdir(parents=True, exist_ok=True)
    # A report left from an earlier run must never stand in for this one.
    for stale in [OUTPUT / "summary.md"] + [OUTPUT / f"{s.mutation}.md" for s in chosen if s.mutation]:
        stale.unlink(missing_ok=True)
    if rerun:
        try:
            collect(chosen)
        except (Exception, SystemExit) as error:
            (OUTPUT / "summary.md").write_text(f"Parity did not finish: {error}\n", encoding="utf-8")
            print(f"parity did not finish: {error}", file=sys.stderr)
            return 2 if isinstance(error, SystemExit) else 1
    reports = evaluate(chosen)
    for report in reports:
        print(f"   {report.scenario} / {report.patch}: {'agree' if report.agree else 'DISAGREE'}")
    summary = write_summary(reports, time.monotonic() - started)
    print("\n" + summary)
    return 0 if all(report.agree for report in reports) else 1


# ------------------------------------------------------------------------- CI

# A pull request changing any of these runs parity in CI: everything that
# decides what either twin does. Entries ending in "/" are directories.
PARITY_PATHS = (
    "src/", "data/", "build.rs", "Cargo.toml", "Cargo.lock",
    "tests/e2e/", "tests/qa/", "tests/qa.rs", "tests/fixtures/",
    "scripts/qa.sh", "scripts/qa-parity.sh", "scripts/qa_parity.py",
    "scripts/e2e.sh", "scripts/e2e-native.sh", "scripts/e2e_base.py",
    "scripts/e2e_bundle.py", "scripts/e2e_images.py", ".github/workflows/ci.yml",
)
# A shard rebuilds both sides for its baseline and again for each patch, so
# more shards finish sooner at the cost of more runners.
CI_SHARDS = 10
# `qa-parity-<attempt>-shard-<index>`; a re-run attempt replaces only its own shards.
SHARD_DIRECTORY = re.compile(r"(?:(\d+)-)?shard-(\d+)$")
SHARD_MINUTES = re.compile(r"\(([\d.]+) minutes\)")


def touches_parity(paths: list[str]) -> bool:
    return any(
        path == prefix or (prefix.endswith("/") and path.startswith(prefix))
        for path in paths
        for prefix in PARITY_PATHS
    )


def ci_patches() -> list[str]:
    """Every scenario's mutation, which CI shards by; each scenario needs its own."""

    found = scenarios()
    if not found:
        raise SystemExit("no QA scenarios to compare")
    unnamed = [scenario.name for scenario in found if not scenario.mutation]
    if unnamed:
        raise SystemExit(f"scenarios without a mutation cannot be sharded: {', '.join(unnamed)}")
    patches = sorted(scenario.mutation for scenario in found)
    if len(set(patches)) != len(patches):
        raise SystemExit("two QA scenarios name the same mutation")
    unknown = [patch for patch in patches if not (MUTATIONS / f"{patch}.patch").is_file()]
    if unknown:
        raise SystemExit(f"unknown mutations: {', '.join(unknown)}")
    return patches


def shard_patches(patches: list[str], shards: int) -> list[list[str]]:
    count = min(shards, len(patches))
    return [patches[index::count] for index in range(count)]


def ci_plan(base: str | None) -> int:
    """GitHub outputs: whether parity runs, and the shard matrix."""

    shards = shard_patches(ci_patches(), CI_SHARDS)
    run = True
    if base is not None:
        changed = git("-c", "core.quotePath=false", "diff", "--no-renames", "--name-only", base, "HEAD")
        if changed.returncode:
            raise SystemExit(f"cannot list the changes since {base}: {changed.stderr.strip()}")
        run = touches_parity(changed.stdout.splitlines())
    matrix = {"include": [{"shard": index, "patches": " ".join(patches)}
                          for index, patches in enumerate(shards)]}
    print(f"run={'true' if run else 'false'}")
    print("matrix=" + json.dumps(matrix, separators=(",", ":")))
    return 0


def aggregate(root: Path) -> int:
    """One verdict over the CI shards' target/qa-parity trees, saved under `root`
    in directories ending `[<attempt>-]shard-<index>`; each shard's newest attempt
    counts, and a shard that uploaded nothing or did not finish disagrees.
    The combined summary goes to `root/summary.md`, leaving target/qa-parity alone."""

    by_patch = {scenario.mutation: scenario for scenario in scenarios()}
    newest: dict[int, tuple[int, Path]] = {}
    for directory in sorted(root.iterdir()) if root.is_dir() else []:
        if directory.is_dir() and (match := SHARD_DIRECTORY.search(directory.name)):
            attempt, index = int(match[1] or 0), int(match[2])
            if index not in newest or attempt > newest[index][0]:
                newest[index] = (attempt, directory)
    uploaded = {index: directory for index, (_, directory) in newest.items()}
    unfinished = False
    reports: list[PairReport] = []
    rows = ["", "| Shard | Patches | Minutes | Result |", "| --- | --- | --- | --- |"]
    for index, patches in enumerate(shard_patches(ci_patches(), CI_SHARDS)):
        chosen = [by_patch[patch] for patch in patches]
        directory = uploaded.pop(index, None)
        summary = directory / "summary.md" if directory else None
        if summary is None or not summary.is_file():
            note = f"shard {index} uploaded no parity report"
            reports += [PairReport(s.name, patch, {"pass": Comparison(False, note)})
                        for s, patch in zip(chosen, patches)]
            rows.append(f"| {index} | {' '.join(patches)} | – | {note} |")
            continue
        text = summary.read_text(encoding="utf-8")
        found = evaluate(chosen, directory)
        reports += found
        minutes = SHARD_MINUTES.search(text)
        result = "agree" if all(report.agree for report in found) else "DISAGREE"
        if text.startswith("Parity did not finish"):
            unfinished = True
            result = f"DISAGREE: {first_line(text)}"
        rows.append(f"| {index} | {' '.join(patches)} | {minutes[1] if minutes else '–'} | "
                    f"{result.replace('|', '/')} |")
    for index in sorted(uploaded):
        rows.append(f"| {index} | – | – | DISAGREE: not in this checkout's shard plan |")
    text = write_summary(reports, None, rows, root)
    print(text)
    if path := os.environ.get("GITHUB_STEP_SUMMARY"):
        with Path(path).open("a", encoding="utf-8") as stream:
            stream.write("## QA parity\n\n" + text)
    return 0 if all(report.agree for report in reports) and not uploaded and not unfinished else 1


def verdict(stage: str, qa_log: Path, junit: Path, qa_status: int) -> int:
    """Pass or fail parity for `e2e-mutation-check.sh --qa`."""

    qa = parse_qa_log(qa_log.read_text(encoding="utf-8", errors="replace"))
    found = pairs(qa, parse_junit(junit))
    for pair in found:
        print(f"   {pair.label} ({pair.twin}): QA {pair.qa.status}, Python {pair.python.status}")
    comparison = pass_parity(found) if stage == "baseline" else fail_parity(found)
    exited = exit_accounted(qa_status, qa.values())
    if comparison.agree and not exited.agree:
        comparison = exited
    print(f"   {'agree' if comparison.agree else 'DISAGREE'}: {comparison.note}")
    return 0 if comparison.agree else 1


def main(arguments: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest="command", required=True)
    run = commands.add_parser("run", help="full parity for every patch, or the ones named")
    run.add_argument("patches", nargs="*")
    again = commands.add_parser("report", help="compare the runs already saved in target/qa-parity")
    again.add_argument("patches", nargs="*")
    one = commands.add_parser("scenario", help="the QA scenario a patch belongs to")
    one.add_argument("patch")
    twins = commands.add_parser("twins", help="the planned runs' twin node ids from a QA log")
    twins.add_argument("log", type=Path)
    check = commands.add_parser("verdict", help="pass or fail parity of one QA log and JUnit report")
    check.add_argument("stage", choices=("baseline", "mutated"))
    check.add_argument("qa_log", type=Path)
    check.add_argument("junit", type=Path)
    check.add_argument("qa_status", type=int, help="qa.sh's exit status")
    plan = commands.add_parser("ci-plan", help="CI: whether parity runs, and the shard matrix")
    plan.add_argument("--base", help="run only if the changes since this commit touch PARITY_PATHS")
    combine = commands.add_parser("aggregate", help="CI: one verdict over the shards' saved runs")
    combine.add_argument("root", type=Path)
    options = parser.parse_args(arguments)

    if options.command == "ci-plan":
        return ci_plan(options.base)
    if options.command == "aggregate":
        return aggregate(options.root)

    if options.command == "scenario":
        print(scenario_for(options.patch).name)
        return 0
    if options.command == "twins":
        found = parse_qa_log(options.log.read_text(encoding="utf-8", errors="replace"))
        print("\n".join(found))
        return 0 if found else 1
    if options.command == "verdict":
        return verdict(options.stage, options.qa_log, options.junit, options.qa_status)
    if options.command == "report":
        return parity(options.patches, rerun=False)

    def terminate(_number, _frame):
        raise KeyboardInterrupt
    signal.signal(signal.SIGTERM, terminate)
    if os.environ.get("STRATA_BINARY") or os.environ.get("STRATA_E2E_BUNDLE"):
        print("STRATA_BINARY and STRATA_E2E_BUNDLE must be unset; parity builds in the container", file=sys.stderr)
        return 2
    try:
        return parity(options.patches)
    except KeyboardInterrupt:
        print("interrupted", file=sys.stderr)
        return 130


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
