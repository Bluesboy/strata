# SPDX-License-Identifier: MIT
"""An isolated, deterministic HOME and XDG environment for one test."""

from __future__ import annotations

import hashlib
import os
import shutil
import tempfile
from dataclasses import dataclass, field
from pathlib import Path

from .fixtures import PARITY_FIXTURES_VARIABLE
from .manifest import MANIFEST

# Strata records that it has already offered to become the file-chooser
# portal by creating this file in its config directory.
PORTAL_OFFER_MARKER: str = MANIFEST["portal_offer_marker"]

GTK_SETTINGS: str = MANIFEST["gtk_settings"]

# Every preference the browser reads at startup, pinned so a scenario never
# inherits a default change. Scenario-specific values are layered on top.
DEFAULT_PREFERENCES: dict[str, object] = MANIFEST["preferences"]

# Locale, rendering, and backend pins; the per-test HOME and XDG paths are
# added by `TestEnvironment.variables`.
STATIC_VARIABLES: dict[str, str] = MANIFEST["environment"]


def parity_environment_name(node_id: str) -> str:
    """`parity_name("strata-home-", ...)` in `tests/qa.rs` derives the same name."""

    return "strata-home-" + hashlib.sha256(node_id.encode()).hexdigest()[:12]


def process_environment() -> dict[str, str]:
    # Do not inherit desktop endpoints, GTK modules, or user configuration overrides.
    return {"PATH": os.environ.get("PATH", os.defpath)}


def _render_toml(values: dict[str, object]) -> str:
    lines = []
    for key, value in values.items():
        lines.append(f"{key} = {_render_value(value)}")
    return "\n".join(lines) + "\n"


def _render_value(value: object) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, (int, float)):
        return repr(value)
    if isinstance(value, list):
        return "[" + ", ".join(_render_value(item) for item in value) + "]"
    return '"' + str(value).replace("\\", "\\\\").replace('"', '\\"') + '"'


@dataclass
class TestEnvironment:
    """A throwaway HOME the application under test may write to freely."""

    root: Path = field(init=False)
    # A display's own helper environment, which keeps a random name under
    # parity so the scenario's environment can take the test's.
    helper: bool = False

    def __post_init__(self) -> None:
        # pytest names the running test as "<node id> (<phase>)". Under
        # parity the root is named after it, as the `--qa` twin names its
        # own, so a typed home path is the same on both sides.
        current = os.environ.get("PYTEST_CURRENT_TEST")
        if not self.helper and os.environ.get(PARITY_FIXTURES_VARIABLE) == "1" and current:
            self.root = Path("/tmp") / parity_environment_name(current.rsplit(" ", 1)[0])
            self.root.mkdir(mode=0o700)
        else:
            self.root = Path(tempfile.mkdtemp(prefix="strata-e2e-home-", dir="/tmp"))
        for name in ("home", "config", "data", "cache", "state"):
            (self.root / name).mkdir()
        gtk = self.config_home / "gtk-4.0"
        gtk.mkdir(parents=True)
        (gtk / "settings.ini").write_text(GTK_SETTINGS)
        self.write_preferences({})
        self._dismiss_portal_offer()

    @property
    def home(self) -> Path:
        return self.root / "home"

    @property
    def config_home(self) -> Path:
        return self.root / "config"

    @property
    def data_home(self) -> Path:
        return self.root / "data"

    @property
    def cache_home(self) -> Path:
        return self.root / "cache"

    @property
    def state_home(self) -> Path:
        return self.root / "state"

    @property
    def settings_path(self) -> Path:
        return self.config_home / "strata" / "settings.toml"

    @property
    def trash_files(self) -> Path:
        return self.data_home / "Trash" / "files"

    def _dismiss_portal_offer(self) -> None:
        """Pre-answer the first-run file-chooser portal offer.

        Strata shows it once per config home; leaving it armed would put a
        modal dialog over every scenario.
        """

        marker = self.config_home / "strata" / PORTAL_OFFER_MARKER
        marker.parent.mkdir(parents=True, exist_ok=True)
        marker.write_text("1\n")

    def write_preferences(self, overrides: dict[str, object]) -> None:
        """Seed `settings.toml`. Must run before the application starts."""

        values = dict(DEFAULT_PREFERENCES)
        values.update(overrides)
        self.settings_path.parent.mkdir(parents=True, exist_ok=True)
        self.settings_path.write_text(_render_toml(values))

    def read_preferences(self) -> dict[str, str]:
        """The persisted preferences as raw `key = value` strings."""

        try:
            text = self.settings_path.read_text()
        except OSError:
            return {}
        values: dict[str, str] = {}
        for line in text.splitlines():
            key, separator, value = line.partition("=")
            if separator:
                values[key.strip()] = value.strip()
        return values

    def variables(self) -> dict[str, str]:
        """Environment variables that pin locale, rendering, and storage."""

        return {
            "HOME": str(self.home),
            "XDG_CONFIG_HOME": str(self.config_home),
            "XDG_DATA_HOME": str(self.data_home),
            "XDG_CACHE_HOME": str(self.cache_home),
            "XDG_STATE_HOME": str(self.state_home),
            **STATIC_VARIABLES,
        }

    def cleanup(self) -> None:
        shutil.rmtree(self.root, ignore_errors=True)
