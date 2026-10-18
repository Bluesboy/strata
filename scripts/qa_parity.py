#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Proves the `--qa` scenarios and their Python twins agree.

For every scenario in tests/qa/scenarios/<twin test file>/ (or the ones named, each by its
`mutation` patch or by its own name), inside the pinned container runners and
one after the other:

- pass parity: both sides pass on the unmodified tree, run by run;
- fail parity: under the scenario's mutation patch, each run fails on both
  sides or on neither, and at least one run fails on both;
- failure-signature parity: where both fail, the Python wait message equals
  the `message` of the QA step that failed;
- input parity: the key and pointer-button lines of the two input logs agree
  in order, pointer positions within one pixel;
- final-state parity: the compared keys of the final state snapshots agree.

A scenario without a `mutation`, or any scenario under `--baseline`, gets
baseline parity: pass, input, and state only.

QA runs map to pytest node ids through the `(twin ...)` on their PASS and
failed lines. Reports go to target/qa-parity: one `<unit>.md` per scenario,
named after its patch or else after the scenario, and `summary.md`. See
"QA parity" in docs/e2e-testing.md.

`scripts/e2e-mutation-check.sh --qa` reuses the `scenario`, `twins`, and
`verdict` subcommands for pass and fail parity alone.
"""

from __future__ import annotations

import argparse
import hashlib
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
# What a scenario's `state_parity_ignore` may name: a compared key, or a
# compared key of each pane.
IGNORABLE_STATE = COMPARED_STATE + ("panes.selected", "panes.focused")
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
INPUT_MOTION = re.compile(r"^motion (-?\d+) (-?\d+)$")


# ------------------------------------------------------------------ scenarios

# The scenario steps that send synthetic input; a scenario with none of them
# agrees on input when neither side logged any.
INPUT_COMMANDS = frozenset({
    "key", "type", "click", "click_result_rows", "select", "select_with_keyboard",
    "open_menu", "move", "move_by", "wheel", "drag", "button_down", "button_up",
    "key_down", "key_up", "keys_until", "drop_samples",
})


@dataclass(frozen=True)
class Scenario:
    name: str
    mutation: str | None
    state_parity_ignore: tuple[str, ...] = ()
    sends_input: bool = True
    # Per run label, the (1-based number, expected event) pairs whose pointer
    # position is not compared.
    input_parity_ignore: tuple[tuple[str, tuple[tuple[int, str], ...]], ...] = ()

    @property
    def unit(self) -> str:
        """What names the scenario on the command line, in reports, and in CI shards."""

        return self.mutation or self.name


def scenarios() -> list[Scenario]:
    found = []
    for path in sorted(SCENARIOS.glob("*/*.jsonl")):
        header = json.loads(path.read_text(encoding="utf-8").splitlines()[0])
        try:
            ignored = state_parity_ignore(header)
            positions = input_parity_ignore(header)
        except ValueError as error:
            raise SystemExit(f"{path}: {error}") from None
        found.append(
            Scenario(path.stem, header.get("mutation"), ignored, sends_input(path), positions)
        )
    return found


def sends_input(path: Path) -> bool:
    """Whether any step of the scenario sends synthetic input."""

    steps = path.read_text(encoding="utf-8").splitlines()[1:]
    return any(
        INPUT_COMMANDS & json.loads(line).keys() for line in steps if line.strip()
    )


def run_labels(header: dict) -> list[str]:
    """The run labels `tests/qa.rs` gives a scenario's runs: the mode, plus `-<variant>`."""

    variants = header.get("variants") or []
    if not variants:
        return list(header.get("modes", []))
    return [
        f"{mode}-{variant['id']}"
        for variant in variants
        for mode in (variant.get("modes") or header.get("modes", []))
    ]


IGNORED_EVENT = re.compile(r"^button (\d+) (down|up)$")


def input_parity_ignore(header: dict) -> tuple[tuple[str, tuple[tuple[int, str], ...]], ...]:
    """The input events a scenario's input parity compares without their pointer position.

    Each entry names one run and, for each such event, its 1-based number
    among the run's key and pointer events (motion lines aside) and the event
    expected there, such as `button 1 down`; the event itself, and every other
    event, is still compared, so a step added before it cannot shift the
    exemption onto another event. `tests/qa.rs` rejects the same values when
    it loads the scenario.
    """

    entries = header.get("input_parity_ignore", [])
    if not isinstance(entries, list):
        raise ValueError("input_parity_ignore is a list of {run, events} entries")
    if entries and header.get("mutation") is not None:
        raise ValueError(
            "input_parity_ignore is for baseline parity; a scenario with a mutation keeps every check"
        )
    labels = run_labels(header)
    found: list[tuple[str, tuple[tuple[int, str], ...]]] = []
    for entry in entries:
        if not isinstance(entry, dict) or set(entry) != {"run", "events"}:
            raise ValueError("an input_parity_ignore entry is {\"run\": ..., \"events\": [...]}")
        run, events = entry["run"], entry["events"]
        if run not in labels or run in (name for name, _ in found):
            raise ValueError(f"input_parity_ignore names distinct runs of {labels}, not {run!r}")
        if not isinstance(events, list) or not events:
            raise ValueError("input_parity_ignore events name at least one event")
        kept: list[tuple[int, str]] = []
        for event in events:
            if (
                not isinstance(event, dict)
                or set(event) != {"position", "event"}
                or not isinstance(event["position"], int)
                or isinstance(event["position"], bool)
                or event["position"] < 1
                or event["position"] in (position for position, _ in kept)
                or not isinstance(event["event"], str)
                or not IGNORED_EVENT.match(event["event"])
            ):
                raise ValueError(
                    "an input_parity_ignore event is {\"position\": N, \"event\": \"button 1 down\"}"
                    " with a distinct position from 1"
                )
            kept.append((event["position"], event["event"]))
        found.append((run, tuple(kept)))
    return tuple(found)


def state_parity_ignore(header: dict) -> tuple[str, ...]:
    """The compared state a scenario's final-state parity leaves out.

    `tests/qa.rs` rejects the same values when it loads the scenario.
    """

    ignored = header.get("state_parity_ignore", [])
    if not isinstance(ignored, list) or not all(isinstance(key, str) for key in ignored):
        raise ValueError("state_parity_ignore is a list of compared state keys")
    unknown = [key for key in ignored if key not in IGNORABLE_STATE]
    if unknown or len(set(ignored)) != len(ignored):
        raise ValueError(
            f"state_parity_ignore takes distinct keys of {IGNORABLE_STATE}, not {unknown or ignored}"
        )
    if ignored and header.get("mutation") is not None:
        raise ValueError(
            "state_parity_ignore is for baseline parity; a scenario with a mutation keeps every check"
        )
    if set(COMPARED_STATE) <= set(ignored):
        raise ValueError("state_parity_ignore must leave a compared key")
    return tuple(ignored)


def scenario_for(patch: str) -> Scenario:
    matching = [scenario for scenario in scenarios() if scenario.mutation == patch]
    if len(matching) != 1:
        raise SystemExit(f"{patch}: expected one QA scenario naming it as its mutation, found {len(matching)}")
    return matching[0]


def scenario_named(name: str) -> Scenario:
    """The scenario a mutation patch belongs to, or else the scenario of that name."""

    found = scenarios()
    by_patch = [scenario for scenario in found if scenario.mutation == name]
    if len(by_patch) > 1:
        raise SystemExit(f"{name}: {len(by_patch)} QA scenarios name it as their mutation")
    if by_patch:
        return by_patch[0]
    by_name = [scenario for scenario in found if scenario.name == name]
    if not by_name:
        raise SystemExit(f"{name}: neither a scenario's mutation nor a scenario in {SCENARIOS}")
    return by_name[0]


# File names stay well inside the 255-byte limit; a longer node id keeps its
# start and a digest of the whole, as `tests/e2e/conftest.py` names it.
NODE_FILE_NAME_LIMIT = 200
NODE_FILE_NAME_KEPT = 180


def node_file_name(node_id: str) -> str:
    name = UNSAFE_NAME.sub("_", node_id)
    if len(name) <= NODE_FILE_NAME_LIMIT:
        return name
    digest = hashlib.sha256(node_id.encode()).hexdigest()[:16]
    return f"{name[:NODE_FILE_NAME_KEPT]}-{digest}"


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


def compare_input(
    python: str, qa: str, sends_input: bool = True, positions: tuple[tuple[int, str], ...] = ()
) -> Comparison:
    """Key and pointer-button lines in order; pointer positions within a pixel.

    A run that only moves the pointer, as a hover does, compares its motion
    lines the same way instead. Two empty logs agree only for a scenario that
    sends no input at all. Each `(number, "button 1 down")` in `positions` must
    be that pointer event on both sides, and is compared without its coordinates.
    """

    def events(text: str, side: str) -> tuple[list[tuple], list[tuple]]:
        found, motion = [], []
        for line in text.splitlines():
            line = line.strip()
            if match := INPUT_KEY.match(line):
                found.append(("key", int(match[1]), match[2]))
            elif match := INPUT_POINTER.match(line):
                found.append(("pointer", int(match[1]), int(match[2]), int(match[3]), match[4]))
            elif match := INPUT_MOTION.match(line):
                motion.append(("motion", int(match[1]), int(match[2])))
            elif line:
                raise ValueError(f"{side} logged an unrecognised line: {line!r}")
        return found, motion

    try:
        (left, left_motion), (right, right_motion) = events(python, "Python"), events(qa, "QA")
    except ValueError as error:
        return Comparison(False, str(error))
    if not left and not right:
        if not left_motion and not right_motion:
            if not sends_input:
                return Comparison(True, "neither side sends input")
            return Comparison(False, "neither side logged input")
        left, right = left_motion, right_motion
    expected = dict(positions)
    if expected and set(expected) >= set(range(1, len(left) + 1)):
        return Comparison(False, "input_parity_ignore leaves no event position compared")
    for number, event in expected.items():
        match = IGNORED_EVENT.match(event)
        wanted = ("pointer", int(match[1]), match[2]) if match else None
        for side, events in (("Python", left), ("QA", right)):
            if number > len(events):
                return Comparison(
                    False, f"input_parity_ignore names event {number}, past {side}'s {len(events)}"
                )
            found = events[number - 1]
            if (found[0], *found[3:]) != wanted:
                return Comparison(
                    False,
                    f"input_parity_ignore expects {event!r} as event {number}; {side} sent {found}",
                )
    for index, (one, other) in enumerate(zip(left, right)):
        if index + 1 in expected:
            continue
        if one[0] != other[0]:
            return Comparison(False, f"event {index + 1}: Python {one}, QA {other}")
        if one[0] == "key" and one != other:
            return Comparison(False, f"event {index + 1}: Python {one}, QA {other}")
        if one[0] in ("pointer", "motion") and (
            one[3:] != other[3:]
            or abs(one[1] - other[1]) > PIXEL_TOLERANCE
            or abs(one[2] - other[2]) > PIXEL_TOLERANCE
        ):
            return Comparison(False, f"event {index + 1}: Python {one}, QA {other}")
    if len(left) != len(right):
        return Comparison(False, f"Python sent {len(left)} events, QA {len(right)}")
    return Comparison(True, f"{len(left)} events")


def compare_state(python: dict | None, qa: dict | None, ignore: tuple[str, ...] = ()) -> Comparison:
    if not python or "error" in python:
        return Comparison(False, f"no Python state: {(python or {}).get('error', 'missing')}")
    if not qa:
        return Comparison(False, "no QA state")
    for side, state in (("Python", python), ("QA", qa)):
        absent = [key for key in COMPARED_STATE if key not in state]
        if absent:
            return Comparison(False, f"the {side} state lacks {', '.join(absent)}")

    def compared(state: dict, key: str):
        value = state.get(key)
        pane_keys = [name.split(".", 1)[1] for name in ignore if name.startswith(f"{key}.")]
        if pane_keys and isinstance(value, list):
            value = [
                {field: item for field, item in pane.items() if field not in pane_keys}
                for pane in value
            ]
        return value

    differing = [
        key for key in COMPARED_STATE
        if key not in ignore and compared(python, key) != compared(qa, key)
    ]
    ignoring = f"; ignoring {', '.join(ignore)}" if ignore else ""
    if differing:
        return Comparison(
            False,
            "; ".join(f"{key}: Python {python.get(key)!r}, QA {qa.get(key)!r}" for key in differing)
            + ignoring,
        )
    return Comparison(True, ignoring.removeprefix("; "))


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

CHECKS = ("pass", "fail", "signature", "input", "state")
# What a scenario without a mutation, or a `--baseline` run, is held to.
BASELINE_CHECKS = ("pass", "input", "state")


@dataclass
class PairReport:
    scenario: str
    patch: str | None
    results: dict[str, Comparison] = field(default_factory=dict)
    baseline: list[RunPair] = field(default_factory=list)
    mutated: list[RunPair] = field(default_factory=list)
    inputs: dict[str, Comparison] = field(default_factory=dict)
    states: dict[str, Comparison] = field(default_factory=dict)
    baseline_only: bool = False

    @property
    def unit(self) -> str:
        return self.patch or self.scenario

    @property
    def checks(self) -> tuple[str, ...]:
        return BASELINE_CHECKS if self.baseline_only else CHECKS

    @property
    def agree(self) -> bool:
        return all(
            check in self.results and self.results[check].agree for check in self.checks
        ) and all(comparison.agree for comparison in self.results.values())


def compare_baseline(report: PairReport, destination: Path) -> None:
    artifacts = destination / "qa"
    python = destination / "python"
    known = {scenario.name: scenario for scenario in scenarios()}
    positions = {
        (name, run): events
        for name, scenario in known.items()
        for run, events in scenario.input_parity_ignore
    }
    ignored = {name: scenario.state_parity_ignore for name, scenario in known.items()}
    for pair in report.baseline:
        if pair.label == "(no QA run)":
            continue
        scenario, run = pair.label.split("/", 1)
        stem = node_file_name(pair.twin)
        python_input = python / "input" / f"{stem}.log"
        qa_input = artifacts / scenario / run / "input.log"
        report.inputs[pair.label] = (
            compare_input(
                python_input.read_text(),
                qa_input.read_text(),
                known[scenario].sends_input if scenario in known else True,
                positions.get((scenario, run), ()),
            )
            if python_input.is_file() and qa_input.is_file()
            else Comparison(False, "an input log is missing")
        )
        python_state = python / "state" / f"{stem}.json"
        qa_state = artifacts / scenario / run / "state.json"
        report.states[pair.label] = compare_state(
            json.loads(python_state.read_text())["app"] if python_state.is_file() else None,
            json.loads(qa_state.read_text()) if qa_state.is_file() else None,
            ignored.get(scenario, ()),
        )

    def combined(comparisons: dict[str, Comparison]) -> Comparison:
        bad = [f"{label}: {item.note}" for label, item in comparisons.items() if not item.agree]
        if not comparisons:
            return Comparison(False, "nothing compared")
        return Comparison(not bad, "; ".join(bad) or f"{len(comparisons)} runs")

    report.results["input"] = combined(report.inputs)
    report.results["state"] = combined(report.states)


def mark(comparison: Comparison | None, applies: bool = True) -> str:
    if not applies:
        return "n/a"
    if comparison is None:
        return "not run"
    return "agree" if comparison.agree else "DISAGREE"


def write_patch_report(report: PairReport, output: Path = OUTPUT) -> Path:
    def cell(text: str) -> str:
        return text.replace("|", "\\|").replace("\n", " ") or "–"

    if report.patch:
        source = f"patch `tests/e2e/mutations/{report.patch}.patch`"
        if report.baseline_only:
            source += ", baseline parity only"
    else:
        source = "no mutation patch, so baseline parity only"
    lines = [
        f"# {report.unit}",
        "",
        f"QA scenario `{report.scenario}`; {source}.",
        "",
        "| Check | Result | Notes |",
        "| --- | --- | --- |",
    ]
    for check in CHECKS:
        comparison = report.results.get(check)
        applies = check in report.checks
        note = comparison.note if comparison and applies else ""
        lines.append(f"| {check} | {mark(comparison, applies)} | {cell(note)} |")
    if not report.baseline_only:
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
    path = output / f"{report.unit}.md"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return path


def write_summary(reports: list[PairReport], elapsed: float | None, extra: list[str] = (),
                  output: Path | None = None) -> str:
    header = "| Scenario | Patch | Runs | Pass | Fail | Signature | Input | State |"
    lines = [header, "| --- | --- | --- | --- | --- | --- | --- | --- |"]
    for report in reports:
        lines.append(
            f"| {report.scenario} | {report.patch or '–'} | {len(report.baseline)} | "
            + " | ".join(mark(report.results.get(check), check in report.checks) for check in CHECKS)
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

def chosen_scenarios(names: list[str]) -> list[Scenario]:
    if names:
        return [scenario_named(name) for name in names]
    return scenarios()


def baseline_pairs(scenario: Scenario, qa: dict[str, QaRun], python: dict[str, Result]) -> list[RunPair]:
    own = {twin: run for twin, run in qa.items() if run.scenario == scenario.name}
    return pairs(own, {twin: python.get(twin, Result("missing")) for twin in own})


def collect(chosen: list[Scenario], baseline_only: bool = False) -> None:
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
        if not scenario.mutation or baseline_only:
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


def evaluate(chosen: list[Scenario], output: Path = OUTPUT, baseline_only: bool = False) -> list[PairReport]:
    """Compares what `collect` saved under `output`; writes `<unit>.md` there for each pair."""

    baseline = output / "baseline"
    qa, python = load_qa(baseline), load_python(baseline)
    baseline_exit = exit_accounted(load_qa_status(baseline), qa.values())
    reports = []
    for scenario in chosen:
        report = PairReport(
            scenario.name, scenario.mutation, baseline_only=baseline_only or not scenario.mutation
        )
        report.baseline = baseline_pairs(scenario, qa, python)
        report.results["pass"] = pass_parity(report.baseline)
        if report.results["pass"].agree and not baseline_exit.agree:
            report.results["pass"] = baseline_exit
        compare_baseline(report, baseline)
        if report.baseline_only:
            write_patch_report(report, output)
            reports.append(report)
            continue
        destination = output / "mutations" / scenario.mutation
        if not (destination / "qa.log").is_file():
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
        write_patch_report(report, output)
        reports.append(report)
    return reports


def parity(names: list[str], rerun: bool = True, baseline_only: bool = False) -> int:
    started = time.monotonic()
    chosen = chosen_scenarios(names)
    OUTPUT.mkdir(parents=True, exist_ok=True)
    # A report left from an earlier run must never stand in for this one.
    for stale in [OUTPUT / "summary.md"] + [OUTPUT / f"{scenario.unit}.md" for scenario in chosen]:
        stale.unlink(missing_ok=True)
    if rerun:
        try:
            collect(chosen, baseline_only)
        except (Exception, SystemExit) as error:
            (OUTPUT / "summary.md").write_text(f"Parity did not finish: {error}\n", encoding="utf-8")
            print(f"parity did not finish: {error}", file=sys.stderr)
            return 2 if isinstance(error, SystemExit) else 1
    reports = evaluate(chosen, baseline_only=baseline_only)
    for report in reports:
        print(f"   {report.scenario} / {report.patch or 'baseline'}: {'agree' if report.agree else 'DISAGREE'}")
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


def ci_units() -> list[str]:
    """What CI shards by: each mutation scenario's patch, which gets full
    parity, and each other scenario's name, which gets baseline parity."""

    found = scenarios()
    if not found:
        raise SystemExit("no QA scenarios to compare")
    patches = [scenario.mutation for scenario in found if scenario.mutation]
    if len(set(patches)) != len(patches):
        raise SystemExit("two QA scenarios name the same mutation")
    unknown = [patch for patch in patches if not (MUTATIONS / f"{patch}.patch").is_file()]
    if unknown:
        raise SystemExit(f"unknown mutations: {', '.join(unknown)}")
    clashing = sorted({scenario.name for scenario in found if not scenario.mutation} & set(patches))
    if clashing:
        raise SystemExit(f"a scenario is named like another's mutation: {', '.join(clashing)}")
    return sorted(scenario.unit for scenario in found)


def shard_units(units: list[str], shards: int) -> list[list[str]]:
    count = min(shards, len(units))
    return [units[index::count] for index in range(count)]


def ci_plan(base: str | None) -> int:
    """GitHub outputs: whether parity runs, and the shard matrix."""

    shards = shard_units(ci_units(), CI_SHARDS)
    run = True
    if base is not None:
        changed = git("-c", "core.quotePath=false", "diff", "--no-renames", "--name-only", base, "HEAD")
        if changed.returncode:
            raise SystemExit(f"cannot list the changes since {base}: {changed.stderr.strip()}")
        run = touches_parity(changed.stdout.splitlines())
    matrix = {"include": [{"shard": index, "names": " ".join(names)}
                          for index, names in enumerate(shards)]}
    print(f"run={'true' if run else 'false'}")
    print("matrix=" + json.dumps(matrix, separators=(",", ":")))
    return 0


def aggregate(root: Path) -> int:
    """One verdict over the CI shards' target/qa-parity trees, saved under `root`
    in directories ending `[<attempt>-]shard-<index>`; each shard's newest attempt
    counts, and a shard that uploaded nothing or did not finish disagrees.
    The combined summary goes to `root/summary.md`, leaving target/qa-parity alone."""

    by_unit = {scenario.unit: scenario for scenario in scenarios()}
    newest: dict[int, tuple[int, Path]] = {}
    for directory in sorted(root.iterdir()) if root.is_dir() else []:
        if directory.is_dir() and (match := SHARD_DIRECTORY.search(directory.name)):
            attempt, index = int(match[1] or 0), int(match[2])
            if index not in newest or attempt > newest[index][0]:
                newest[index] = (attempt, directory)
    uploaded = {index: directory for index, (_, directory) in newest.items()}
    unfinished = False
    reports: list[PairReport] = []
    rows = ["", "| Shard | Scenarios | Minutes | Result |", "| --- | --- | --- | --- |"]
    for index, units in enumerate(shard_units(ci_units(), CI_SHARDS)):
        chosen = [by_unit[unit] for unit in units]
        directory = uploaded.pop(index, None)
        summary = directory / "summary.md" if directory else None
        if summary is None or not summary.is_file():
            note = f"shard {index} uploaded no parity report"
            reports += [PairReport(scenario.name, scenario.mutation, {"pass": Comparison(False, note)},
                                   baseline_only=not scenario.mutation)
                        for scenario in chosen]
            rows.append(f"| {index} | {' '.join(units)} | – | {note} |")
            continue
        text = summary.read_text(encoding="utf-8")
        found = evaluate(chosen, directory)
        reports += found
        minutes = SHARD_MINUTES.search(text)
        result = "agree" if all(report.agree for report in found) else "DISAGREE"
        if text.startswith("Parity did not finish"):
            unfinished = True
            result = f"DISAGREE: {first_line(text)}"
        rows.append(f"| {index} | {' '.join(units)} | {minutes[1] if minutes else '–'} | "
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
    run = commands.add_parser(
        "run", help="parity for every scenario, or the ones named by patch or scenario name"
    )
    run.add_argument("names", nargs="*")
    run.add_argument("--baseline", action="store_true", help="baseline parity only, no patches")
    baseline = commands.add_parser("baseline", help="baseline parity for every scenario, or the ones named")
    baseline.add_argument("names", nargs="*")
    again = commands.add_parser("report", help="compare the runs already saved in target/qa-parity")
    again.add_argument("names", nargs="*")
    again.add_argument("--baseline", action="store_true", help="baseline parity only, no patches")
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
    baseline_only = options.command == "baseline" or getattr(options, "baseline", False)
    if options.command == "report":
        return parity(options.names, rerun=False, baseline_only=baseline_only)

    def terminate(_number, _frame):
        raise KeyboardInterrupt
    signal.signal(signal.SIGTERM, terminate)
    if os.environ.get("STRATA_BINARY") or os.environ.get("STRATA_E2E_BUNDLE"):
        print("STRATA_BINARY and STRATA_E2E_BUNDLE must be unset; parity builds in the container", file=sys.stderr)
        return 2
    try:
        return parity(options.names, baseline_only=baseline_only)
    except KeyboardInterrupt:
        print("interrupted", file=sys.stderr)
        return 130


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
