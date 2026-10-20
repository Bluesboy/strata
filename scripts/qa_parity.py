#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Proves the `--qa` scenarios and their Python twins agree."""

from __future__ import annotations

import argparse
import os
import signal
import sys
from pathlib import Path

from qa_parity_compare import (  # noqa: F401
    EXCEPTION_PREFIX,
    INPUT_KEY,
    INPUT_MOTION,
    INPUT_POINTER,
    NODE_FILE_NAME_KEPT,
    NODE_FILE_NAME_LIMIT,
    PIXEL_TOLERANCE,
    QA_FAILED,
    QA_KNOWN_BUG,
    QA_PASS,
    QA_PLANNED,
    RUN,
    UNSAFE_NAME,
    WAITING_FOR,
    XDIST_GROUP,
    Comparison,
    QaRun,
    Result,
    RunPair,
    add_qa_details,
    compare_input,
    compare_state,
    exit_accounted,
    fail_parity,
    first_line,
    node_file_name,
    normalise,
    pairs,
    parse_junit,
    parse_qa_log,
    pass_parity,
    python_signature,
    qa_signature,
    signature_parity,
)
from qa_parity_run import (  # noqa: F401
    BASELINE_CHECKS,
    CHECKS,
    OUTPUT,
    PARITY_FIXTURES,
    QA_ARTIFACTS,
    Mutation,
    PairReport,
    baseline_pairs,
    collect,
    compare_baseline,
    evaluate,
    git,
    load_python,
    load_qa,
    load_qa_status,
    mark,
    parity,
    patched_files,
    run_logged,
    run_python,
    run_qa,
    write_patch_report,
    write_summary,
)
from qa_parity_scenarios import (  # noqa: F401
    COMPARED_STATE,
    DIGIT_KEYCODES,
    IGNORABLE_STATE,
    IGNORED_EVENT,
    INPUT_COMMANDS,
    MUTATIONS,
    REPOSITORY,
    SCENARIOS,
    WHOLE_STATE,
    Scenario,
    chosen_scenarios,
    input_parity_ignore,
    run_labels,
    scenario_for,
    scenario_named,
    scenarios,
    sends_input,
    skip_reason,
    skipped_lines,
    state_parity_ignore,
)


def verdict(stage: str, qa_log: Path, junit: Path, qa_status: int) -> int:
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
    options = parser.parse_args(arguments)

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
