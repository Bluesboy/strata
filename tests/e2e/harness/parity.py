# SPDX-License-Identifier: MIT
"""Input logs and final-state dumps for QA parity.

Inert unless `STRATA_INPUT_LOG` or `STRATA_STATE_DUMP` names a directory.
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import time
from pathlib import Path
from typing import Callable

from .xtest import INPUT_LOG_VARIABLE, XTestConnection

STATE_DUMP_VARIABLE = "STRATA_STATE_DUMP"
STATE_SETTLE_TIMEOUT = 5.0
STATE_SETTLE_INTERVAL = 0.25
NODE_FILE_NAME_LIMIT = 200
NODE_FILE_NAME_KEPT = 180


def node_file_name(node_id: str) -> str:
    """`scripts/qa_parity_compare.py` derives the same name from a twin's node id."""

    name = re.sub(r"[^A-Za-z0-9._-]+", "_", node_id)
    if len(name) <= NODE_FILE_NAME_LIMIT:
        return name
    digest = hashlib.sha256(node_id.encode()).hexdigest()[:16]
    return f"{name[:NODE_FILE_NAME_KEPT]}-{digest}"


def current_node_id() -> str | None:
    current = os.environ.get("PYTEST_CURRENT_TEST")
    return current.rsplit(" ", 1)[0] if current else None


def start_input_log(
    connection: XTestConnection, node_id: str, origin: Callable[[], tuple[int, int]]
) -> None:
    directory = os.environ.get(INPUT_LOG_VARIABLE)
    if not directory:
        return
    connection.start_input_log(Path(directory) / f"{node_file_name(node_id)}.log", origin)


def settled_snapshot(
    read: Callable[[], dict[str, object]], finished: Callable[[], bool]
) -> dict[str, object]:
    """The snapshot once two reads in a row agree, as the `--qa` runner takes it.

    A node vanishing mid-read only means the state has not settled; once
    `finished()` says the window or its process is gone, a failed read is final.
    """

    deadline = time.monotonic() + STATE_SETTLE_TIMEOUT
    previous = None
    while True:
        try:
            state = read()
        except AssertionError:
            raise
        except Exception:
            if time.monotonic() >= deadline or finished():
                raise
            state = None
        if state is not None and (state == previous or time.monotonic() >= deadline):
            return state
        previous = state
        time.sleep(STATE_SETTLE_INTERVAL)


def dump_state(node_id: str, read: Callable[[], dict[str, object]], finished: Callable[[], bool]) -> None:
    directory = os.environ.get(STATE_DUMP_VARIABLE)
    if not directory:
        return
    try:
        state = settled_snapshot(read, finished)
    except Exception as error:  # the window may already be gone after a failure
        state = {"error": repr(error)}
    path = Path(directory) / f"{node_file_name(node_id)}.json"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps({"nodeid": node_id, "app": state}, indent=2) + "\n")
