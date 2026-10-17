# SPDX-License-Identifier: MIT
"""`tests/fixtures/harness.json` seeds the `--qa` runner (`tests/qa.rs`).

Until this suite reads the manifest itself, it must describe exactly the
fixture tree, preferences, environment, and layouts this harness creates.
"""

from __future__ import annotations

import json
from pathlib import Path

from harness import environment, keyboard_layout
from harness.fixtures import CANONICAL_TREE, FIXTURE_MTIME

MANIFEST = Path(__file__).resolve().parents[2] / "fixtures" / "harness.json"
PER_TEST_PATHS = {"HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME"}


def test_manifest_matches_the_python_fixtures_and_environment():
    manifest = json.loads(MANIFEST.read_text())
    # Imported through its module so pytest does not try to collect it as a test class.
    home = environment.TestEnvironment()
    try:
        variables = home.variables()
    finally:
        home.cleanup()

    assert manifest["mtime"] == FIXTURE_MTIME
    assert manifest["trees"] == {"canonical": CANONICAL_TREE}
    assert list(manifest["preferences"].items()) == list(
        environment.DEFAULT_PREFERENCES.items()
    )
    assert manifest["gtk_settings"] == environment.GTK_SETTINGS
    assert manifest["portal_offer_marker"] == environment.PORTAL_OFFER_MARKER
    assert manifest["environment"] == {
        key: value for key, value in variables.items() if key not in PER_TEST_PATHS
    }
    layouts = {
        layout.name: {
            "layouts": layout.layouts,
            "group": layout.group,
            "caps_lock": layout.caps_lock,
            "variants": layout.variants,
        }
        for layout in vars(keyboard_layout).values()
        if isinstance(layout, keyboard_layout.Layout)
    }
    assert manifest["layouts"] == layouts
