# SPDX-License-Identifier: MIT
"""Running both sides, on the unmodified and the mutated tree, and reporting parity."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path

from qa_parity_compare import (
    Comparison,
    QaRun,
    Result,
    RunPair,
    add_qa_details,
    compare_input,
    compare_state,
    exit_accounted,
    fail_parity,
    node_file_name,
    pairs,
    parse_junit,
    parse_qa_log,
    pass_parity,
    signature_parity,
)
from qa_parity_scenarios import (
    MUTATIONS,
    REPOSITORY,
    Scenario,
    chosen_scenarios,
    scenarios,
    skipped_lines,
)

OUTPUT = REPOSITORY / "target/qa-parity"
QA_ARTIFACTS = REPOSITORY / "target/qa-artifacts"
PARITY_FIXTURES = {"STRATA_PARITY_FIXTURES": "1"}


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


CHECKS = ("pass", "fail", "signature", "input", "state")
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
        stem = node_file_name(pair.python.node or pair.twin)
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


def baseline_pairs(scenario: Scenario, qa: dict[str, QaRun], python: dict[str, Result]) -> list[RunPair]:
    own = {twin: run for twin, run in qa.items() if run.scenario == scenario.name}
    return pairs(own, {twin: python.get(twin, Result("missing")) for twin in own})


def collect(chosen: list[Scenario], baseline_only: bool = False) -> None:
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
    # qa.sh without names would run every scenario.
    if rerun and chosen:
        try:
            collect(chosen, baseline_only)
        except (Exception, SystemExit) as error:
            (OUTPUT / "summary.md").write_text(f"Parity did not finish: {error}\n", encoding="utf-8")
            print(f"parity did not finish: {error}", file=sys.stderr)
            return 2 if isinstance(error, SystemExit) else 1
    reports = evaluate(chosen, baseline_only=baseline_only)
    for report in reports:
        print(f"   {report.scenario} / {report.patch or 'baseline'}: {'agree' if report.agree else 'DISAGREE'}")
    summary = write_summary(reports, time.monotonic() - started, skipped_lines(names))
    print("\n" + summary)
    return 0 if all(report.agree for report in reports) else 1
