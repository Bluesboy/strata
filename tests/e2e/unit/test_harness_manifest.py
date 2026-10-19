# SPDX-License-Identifier: MIT
"""`tests/fixtures/harness.json` is the one definition both harnesses read."""

from __future__ import annotations

import os

from harness import environment, keyboard_layout, manifest
from harness.fixtures import FixtureTree

PER_TEST_PATHS = {"HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME"}


def test_the_fixture_tree_is_materialised_from_the_manifest():
    tree = FixtureTree.create()
    try:
        def read(directory):
            return {
                path.name: read(path) if path.is_dir() else path.read_text()
                for path in directory.iterdir()
            }

        assert read(tree.root) == manifest.MANIFEST["trees"]["canonical"]
        times = {
            int(os.stat(path, follow_symlinks=False).st_mtime)
            for path in [tree.root, *tree.root.rglob("*")]
        }
        assert times == {manifest.MANIFEST["mtime"]}
    finally:
        tree.cleanup()


def test_the_seeded_home_is_written_from_the_manifest():
    # Imported through its module so pytest does not try to collect it as a test class.
    home = environment.TestEnvironment()
    try:
        variables = home.variables()
        settings = home.settings_path.read_text()
        gtk_settings = (home.config_home / "gtk-4.0" / "settings.ini").read_text()
        marker = home.config_home / "strata" / manifest.MANIFEST["portal_offer_marker"]
        assert marker.is_file()
    finally:
        home.cleanup()

    preferences = manifest.MANIFEST["preferences"]
    keys = [line.partition(" = ")[0] for line in settings.splitlines()]
    assert keys == list(preferences), "settings.toml keeps the manifest's order"
    assert all(
        line == f"{key} = {environment._render_value(preferences[key])}"
        for key, line in zip(keys, settings.splitlines())
    )
    assert gtk_settings == manifest.MANIFEST["gtk_settings"]
    assert set(variables) == PER_TEST_PATHS | set(manifest.MANIFEST["environment"])
    assert not PER_TEST_PATHS & set(manifest.MANIFEST["environment"]), (
        "per-test paths are created per run, never pinned in the manifest"
    )


def test_every_manifest_layout_is_a_named_layout():
    named = {
        layout.name
        for layout in vars(keyboard_layout).values()
        if isinstance(layout, keyboard_layout.Layout)
    }
    assert named == set(manifest.MANIFEST["layouts"])
