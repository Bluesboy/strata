# SPDX-License-Identifier: MIT
"""QA scenario headers and the choice of scenarios to compare."""

from __future__ import annotations

import json
import re
from dataclasses import dataclass
from pathlib import Path

REPOSITORY = Path(__file__).resolve().parents[1]
SCENARIOS = REPOSITORY / "tests/qa/scenarios"
MUTATIONS = REPOSITORY / "tests/e2e/mutations"
COMPARED_STATE = ("view_mode", "directory", "panes", "focused", "focused_pane", "dialog")
IGNORABLE_STATE = COMPARED_STATE + ("panes.selected", "panes.focused")
WHOLE_STATE = "*"


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
    input_parity_ignore: tuple[tuple[str, tuple[tuple[int, str], ...]], ...] = ()
    skip: str | None = None
    twin: bool = True

    @property
    def unit(self) -> str:
        return self.mutation or self.name


def scenarios() -> list[Scenario]:
    found = []
    for path in sorted(SCENARIOS.glob("*/*.jsonl")):
        header = json.loads(path.read_text(encoding="utf-8").splitlines()[0])
        try:
            ignored = state_parity_ignore(header)
            positions = input_parity_ignore(header)
            skip = skip_reason(header)
        except ValueError as error:
            raise SystemExit(f"{path}: {error}") from None
        found.append(Scenario(
            path.stem, header.get("mutation"), ignored, sends_input(path), positions, skip,
            header.get("twin") is not None,
        ))
    return found


def skip_reason(header: dict) -> str | None:
    """`tests/qa.rs` rejects the same values when it loads the scenario."""

    reason = header.get("skip")
    if reason is None:
        return None
    if not isinstance(reason, str) or not reason.strip():
        raise ValueError("skip gives the reason the scenario stays out of the run")
    if header.get("mutation") is not None or header.get("known_bug") is not None:
        raise ValueError("skip is for a scenario without a mutation or known_bug")
    return reason


def sends_input(path: Path) -> bool:
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


IGNORED_EVENT = re.compile(r"^(?:button (\d+)|key) (down|up)$|^typed digits$")
# US key positions of the digits 1-9 and 0, as the X server numbers them.
DIGIT_KEYCODES = frozenset(range(10, 20))


def input_parity_ignore(header: dict) -> tuple[tuple[str, tuple[tuple[int, str], ...]], ...]:
    """Input events compared without their position or key; `tests/qa.rs` rejects the same values.

    The event itself is still compared, so a step added before it cannot shift
    the exemption onto another event.
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
                    " (or \"key down\", or \"typed digits\") with a distinct position from 1"
                )
            kept.append((event["position"], event["event"]))
        found.append((run, tuple(kept)))
    return tuple(found)


def state_parity_ignore(header: dict) -> tuple[str, ...]:
    """`tests/qa.rs` rejects the same values when it loads the scenario."""

    ignored = header.get("state_parity_ignore", [])
    if not isinstance(ignored, list) or not all(isinstance(key, str) for key in ignored):
        raise ValueError("state_parity_ignore is a list of compared state keys")
    if ignored == [WHOLE_STATE]:
        if header.get("mutation") is not None:
            raise ValueError(
                "state_parity_ignore is for baseline parity; a scenario with a mutation keeps every check"
            )
        return (WHOLE_STATE,)
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


def chosen_scenarios(names: list[str]) -> list[Scenario]:
    found = [scenario_named(name) for name in names] if names else scenarios()
    return [scenario for scenario in found if scenario.skip is None and scenario.twin]


def skipped_lines(names: list[str]) -> list[str]:
    found = [scenario_named(name) for name in names] if names else scenarios()
    return [
        f"No twin, not compared: {scenario.name}"
        for scenario in found
        if not scenario.twin
    ] + [
        f"Skipped, not compared: {scenario.name} ({scenario.skip})"
        for scenario in found
        if scenario.twin and scenario.skip is not None
    ]
