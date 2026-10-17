# SPDX-License-Identifier: MIT
"""The shared harness manifest, `tests/fixtures/harness.json`.

It is the single definition of the fixture tree, seeded preferences, GTK
settings, static environment, and keyboard layouts. This harness and the
`--qa` runner (`tests/qa.rs`) both read it, so neither depends on two
generators agreeing.
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

PATH = Path(__file__).resolve().parents[2] / "fixtures" / "harness.json"

MANIFEST: dict[str, Any] = json.loads(PATH.read_text(encoding="utf-8"))
