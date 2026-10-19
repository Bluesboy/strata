# SPDX-License-Identifier: MIT
"""The CI shard plan and the verdict over the shards' saved runs."""

from __future__ import annotations

import json
import os
import re
from pathlib import Path

from qa_parity_compare import Comparison, first_line
from qa_parity_run import PairReport, evaluate, git, write_summary
from qa_parity_scenarios import MUTATIONS, scenarios


PARITY_PATHS = (
    "src/", "data/", "build.rs", "Cargo.toml", "Cargo.lock",
    "tests/e2e/", "tests/qa/", "tests/qa.rs", "tests/fixtures/",
    "scripts/qa.sh", "scripts/lib/qa-container.sh", "scripts/qa-parity.sh",
    "scripts/qa_parity.py", "scripts/qa_parity_ci.py", "scripts/qa_parity_compare.py",
    "scripts/qa_parity_run.py", "scripts/qa_parity_scenarios.py",
    "scripts/e2e.sh", "scripts/e2e-native.sh", "scripts/e2e_base.py",
    "scripts/e2e_bundle.py", "scripts/e2e_images.py", ".github/workflows/ci.yml",
)
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
    found = [scenario for scenario in scenarios() if scenario.twin]
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
    return sorted(scenario.unit for scenario in found if scenario.skip is None)


def shard_units(units: list[str], shards: int) -> list[list[str]]:
    count = min(shards, len(units))
    return [units[index::count] for index in range(count)]


def ci_plan(base: str | None) -> int:
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
    """Each shard's newest attempt counts; a shard that uploaded nothing or did not finish disagrees."""

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
