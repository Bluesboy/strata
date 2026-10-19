# SPDX-License-Identifier: MIT
"""Deterministic fixture trees.

Every scenario gets a freshly generated tree inside its own temporary
directory. Nothing outside that directory is read or written.
"""

from __future__ import annotations

import hashlib
import os
import shutil
import tempfile
from dataclasses import dataclass
from pathlib import Path

from .manifest import MANIFEST

# Fixed so mtime-sorted views and golden screenshots stay stable.
FIXTURE_MTIME: int = MANIFEST["mtime"]

# Names are chosen to sort predictably and to avoid colliding with the
# directories a scenario creates.
CANONICAL_TREE: dict[str, object] = MANIFEST["trees"]["canonical"]


# Set to `1` by `scripts/qa_parity.py` to name each tree after its node id, as
# the `--qa` twin names its own.
PARITY_FIXTURES_VARIABLE = "STRATA_PARITY_FIXTURES"


def parity_fixture_name(node_id: str) -> str:
    """`parity_fixture_name` in `tests/qa.rs` derives the same name from the twin."""

    return "strata-fixture-" + hashlib.sha256(node_id.encode()).hexdigest()[:12]


@dataclass
class FixtureTree:
    """A generated directory tree plus helpers for asserting on it."""

    root: Path

    @classmethod
    def create(cls, layout: dict[str, object] | None = None) -> "FixtureTree":
        current = os.environ.get("PYTEST_CURRENT_TEST")
        if os.environ.get(PARITY_FIXTURES_VARIABLE) == "1" and current:
            node_id = current.rsplit(" ", 1)[0]
            return cls.create_at(Path("/tmp") / parity_fixture_name(node_id), layout)
        root = Path(tempfile.mkdtemp(prefix="strata-e2e-fixture-", dir="/tmp"))
        return cls._build(root, layout)

    @classmethod
    def create_at(
        cls, root: Path, layout: dict[str, object] | None = None
    ) -> "FixtureTree":
        """Build the tree at a fixed path.

        Golden screenshots show the directory name and its full path, so the
        scenarios behind them cannot use a randomized temporary directory.
        """

        # Never remove another run's fixture or pre-existing user data.
        root.mkdir(mode=0o700, parents=True)
        return cls._build(root, layout)

    @classmethod
    def _build(
        cls, root: Path, layout: dict[str, object] | None
    ) -> "FixtureTree":
        tree = cls(root=root)
        tree.populate(layout if layout is not None else CANONICAL_TREE)
        return tree

    def populate(self, layout: dict[str, object], parent: Path | None = None) -> None:
        base = parent if parent is not None else self.root
        for name, value in sorted(layout.items()):
            path = base / name
            if isinstance(value, dict):
                path.mkdir(parents=True, exist_ok=True)
                path.chmod(0o755)
                self.populate(value, path)
            else:
                path.write_text(str(value))
                path.chmod(0o644)
        self._pin_times(base)

    def _pin_times(self, base: Path) -> None:
        for path in sorted(base.rglob("*")) + [base]:
            os.utime(path, (FIXTURE_MTIME, FIXTURE_MTIME))

    def path(self, relative: str) -> Path:
        return self.root / relative

    def names(self, relative: str = ".") -> list[str]:
        """Sorted entry names in a directory, hidden files included."""

        directory = self.root / relative
        return sorted(entry.name for entry in directory.iterdir())

    def listing(self) -> str:
        """A stable textual listing, captured with failure artifacts."""

        lines = []
        for path in sorted(self.root.rglob("*")):
            relative = path.relative_to(self.root)
            if path.is_dir():
                lines.append(f"{relative}/")
            elif path.is_symlink():
                lines.append(f"{relative} -> {os.readlink(path)}")
            else:
                lines.append(f"{relative} ({path.stat().st_size} bytes)")
        return "\n".join(lines)

    def cleanup(self) -> None:
        shutil.rmtree(self.root, ignore_errors=True)
