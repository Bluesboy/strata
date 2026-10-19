# SPDX-License-Identifier: MIT
"""Parsing both sides' run results and comparing them."""

from __future__ import annotations

import hashlib
import json
import re
import xml.etree.ElementTree as ET
from dataclasses import dataclass
from pathlib import Path

from qa_parity_scenarios import COMPARED_STATE, DIGIT_KEYCODES, IGNORED_EVENT, WHOLE_STATE

PIXEL_TOLERANCE = 1
# Same file naming as `node_file_name` in tests/e2e/harness/parity.py.
UNSAFE_NAME = re.compile(r"[^A-Za-z0-9._-]+")
# pytest-xdist's `loadgroup` appends `@<group>` to an `xdist_group` test's node id;
# the Python side logs under that id, the QA twin under the bare one.
XDIST_GROUP = re.compile(r"@[^@\[\]/]+$")

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


# Names stay inside the 255-byte limit as `tests/e2e/harness/parity.py` names them.
NODE_FILE_NAME_LIMIT = 200
NODE_FILE_NAME_KEPT = 180


def node_file_name(node_id: str) -> str:
    name = UNSAFE_NAME.sub("_", node_id)
    if len(name) <= NODE_FILE_NAME_LIMIT:
        return name
    digest = hashlib.sha256(node_id.encode()).hexdigest()[:16]
    return f"{name[:NODE_FILE_NAME_KEPT]}-{digest}"


@dataclass
class Result:
    status: str
    signature: str = ""
    detail: str = ""
    node: str = ""


@dataclass
class QaRun:
    scenario: str
    run: str
    twin: str
    result: Result


def parse_qa_log(text: str) -> dict[str, QaRun]:
    """QA runs by twin node id.

    A planned `RUN` without a result line stays `missing`, so a truncated run
    never drops out. Parity does not accept known bugs.
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
    try:
        root = ET.parse(report).getroot()
    except (ET.ParseError, OSError):
        return {}
    results: dict[str, Result] = {}
    for case in root.iter("testcase"):
        classname = case.get("classname", "")
        ran = classname.replace(".", "/") + ".py::" + case.get("name", "")
        node = XDIST_GROUP.sub("", ran)
        failure = case.find("failure")
        error = case.find("error")
        if error is not None:
            result = Result("error", detail=first_line(error.get("message", "")))
        elif failure is not None:
            message = failure.get("message", "")
            result = Result("fail", python_signature(message), first_line(message))
        elif case.find("skipped") is not None:
            result = Result("error", detail="skipped")
        else:
            result = Result("pass")
        if ran != node:
            result.node = ran
        results[node] = result
    return results


def python_signature(message: str) -> str:
    if match := WAITING_FOR.search(message):
        return normalise(match["message"])
    return normalise(EXCEPTION_PREFIX.sub("", first_line(message)))


def first_line(text: str) -> str:
    return text.strip().splitlines()[0] if text.strip() else ""


def normalise(text: str) -> str:
    return " ".join(text.split())


@dataclass
class Comparison:
    agree: bool
    note: str = ""


def compare_input(
    python: str, qa: str, sends_input: bool = True, positions: tuple[tuple[int, str], ...] = ()
) -> Comparison:
    """Key and pointer-button lines in order; pointer positions within a pixel.

    A pointer-only run (a hover) compares its motion lines instead. Two empty
    logs agree only for a scenario that sends no input. Numbers in `positions`
    count Python's events; after `"typed digits"` the logs are compared again
    from the ends of both digit runs.
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

    def digits(events: list[tuple], start: int) -> int:
        end = start
        while (
            end + 1 < len(events)
            and events[end][0] == "key"
            and events[end][1] in DIGIT_KEYCODES
            and events[end][2] == "down"
            and events[end + 1] == ("key", events[end][1], "up")
        ):
            end += 2
        return end - start

    left_index = right_index = 0
    visited = set()
    while left_index < len(left) and right_index < len(right):
        number = left_index + 1
        visited.add(number)
        event = expected.get(number)
        one, other = left[left_index], right[right_index]
        if event == "typed digits":
            spans = digits(left, left_index), digits(right, right_index)
            if not all(spans):
                return Comparison(
                    False,
                    f"input_parity_ignore expects typed digits as event {number}; "
                    f"Python sent {one}, QA {other}",
                )
            left_index += spans[0]
            right_index += spans[1]
            continue
        if event is not None:
            match = IGNORED_EVENT.match(event)
            if match[1]:
                wanted = ("pointer", int(match[1]), match[2])
                shape = lambda found: (found[0], *found[3:])  # noqa: E731
            else:
                wanted = ("key", match[2])
                shape = lambda found: (found[0], found[-1])  # noqa: E731
            for side, found in (("Python", one), ("QA", other)):
                if shape(found) != wanted:
                    return Comparison(
                        False,
                        f"input_parity_ignore expects {event!r} as event {number}; {side} sent {found}",
                    )
        elif one[0] != other[0]:
            return Comparison(False, f"event {number}: Python {one}, QA {other}")
        elif one[0] == "key" and one != other:
            return Comparison(False, f"event {number}: Python {one}, QA {other}")
        elif one[0] in ("pointer", "motion") and (
            one[3:] != other[3:]
            or abs(one[1] - other[1]) > PIXEL_TOLERANCE
            or abs(one[2] - other[2]) > PIXEL_TOLERANCE
        ):
            return Comparison(False, f"event {number}: Python {one}, QA {other}")
        left_index += 1
        right_index += 1
    past = [number for number in expected if number > len(left)]
    if past:
        return Comparison(
            False, f"input_parity_ignore names event {past[0]}, past Python's {len(left)}"
        )
    if expected and not visited - set(expected):
        return Comparison(False, "input_parity_ignore leaves no event position compared")
    skipped = sorted(set(expected) - visited)
    if skipped:
        return Comparison(
            False,
            f"input_parity_ignore names event {skipped[0]}, inside a run of typed digits",
        )
    if left_index < len(left) or right_index < len(right):
        return Comparison(False, f"Python sent {len(left)} events, QA {len(right)}")
    return Comparison(True, f"{len(left)} events")


def compare_state(python: dict | None, qa: dict | None, ignore: tuple[str, ...] = ()) -> Comparison:
    if ignore == (WHOLE_STATE,):
        return Comparison(True, "not compared: state_parity_ignore *")
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


def exit_accounted(status: int | None, runs) -> Comparison:
    if status is None:
        return Comparison(False, "qa.sh's exit status was not recorded")
    failing = any(run.result.status != "pass" for run in runs)
    if (status != 0) != failing:
        return Comparison(False, f"qa.sh exited {status}, which its run results do not account for")
    return Comparison(True)
