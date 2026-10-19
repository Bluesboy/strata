# SPDX-License-Identifier: MIT
"""The opt-in provider against real private D-Bus and Strata's native menus."""

import json
from pathlib import Path
import sys

import pytest

from harness.process import ManagedProcess, terminate

PROVIDER = Path(__file__).resolve().parents[3] / "integrations/kdeconnect"


class Devices:
    def __init__(self, folder):
        self.folder = folder
        self.control = folder / "control.json"
        self.record = folder / "record.jsonl"

    def set(self, devices):
        temporary = self.control.with_suffix(".tmp")
        temporary.write_text(json.dumps({"devices": devices}))
        temporary.replace(self.control)

    def submissions(self):
        return [json.loads(line) for line in self.record.read_text().splitlines()] if self.record.exists() else []


@pytest.fixture(autouse=True)
def kdeconnect(test_environment, headless_display):
    folder = test_environment.config_home / "strata/providers/kdeconnect"
    folder.mkdir(parents=True, mode=0o700)
    devices = Devices(folder)
    devices.set({})
    # Install only in the throwaway test configuration, never in the desktop.
    (folder / "provider.json").write_text(json.dumps({
        "version": 1, "id": "kdeconnect", "name": "KDE Connect", "icons": {},
        "command": ["/usr/bin/python3", str(PROVIDER / "provider.py")],
    }))
    ready = folder / "ready"
    # Its own session, so terminate() stops it before the next test owns the name.
    service = ManagedProcess.spawn(
        "mock",
        [sys.executable, str(PROVIDER / "tests/mock_service.py"),
         str(devices.control), str(devices.record), str(ready)],
        log_dir=folder,
        env={**test_environment.variables(), **headless_display.environment},
    )
    try:
        from harness.tree import wait_until
        wait_until(ready.exists, message="private KDE Connect mock startup", timeout=10)
        yield devices
    finally:
        terminate(service.popen)


def test_devices_are_live_send_to_destinations(strata, kdeconnect):
    strata.open_context_menu("todo.txt")
    strata.wait(lambda: {"Open", "Cut"} <= set(strata.menu_items()), "context menu")
    assert "Send to…" not in strata.menu_items()
    kdeconnect.set({"phone1": {"name": "Pixel"}})
    strata.wait(lambda: "Send to…" in strata.menu_items(), "a reachable device adds Send to…")
    assert "Pixel" not in strata.menu_items(), "devices live under Send to…"
    strata.pointer.click(strata.menu_item("Send to…"))
    strata.wait(lambda: "KDE Connect" in strata.menu_items(), "KDE Connect submenu inside Send to…")
    strata.pointer.click(strata.menu_item("KDE Connect"))
    strata.wait(lambda: "Pixel" in strata.menu_items(), "device inside the KDE Connect submenu")
    strata.choose_menu_item("Pixel")
    strata.wait(lambda: len(kdeconnect.submissions()) == 1, "direct D-Bus submission")
    assert kdeconnect.submissions()[0]["device"] == "phone1"
    assert kdeconnect.submissions()[0]["uris"] == [strata.fixture.path("todo.txt").as_uri()]
    strata.wait(lambda: strata.window.find(role="label", name="Files submitted to Pixel: 1. KDE Connect handles transfer progress and results."), "submission notice")
    assert strata.dialog() is None, "accepted send must not open a picker or success dialog"
    strata.open_context_menu("todo.txt")
    strata.wait(lambda: "Send to…" in strata.menu_items(), "Send to… with one device")
    strata.pointer.click(strata.menu_item("Send to…"))
    strata.wait(lambda: "KDE Connect" in strata.menu_items(), "KDE Connect submenu")
    strata.pointer.click(strata.menu_item("KDE Connect"))
    strata.wait(lambda: "Pixel" in strata.menu_items(), "single device")
    kdeconnect.set({"phone1": {"name": "Pixel"}, "tablet2": {"name": "Tablet"}})
    strata.wait(lambda: "Tablet" in strata.menu_items(), "open Send to… gains a device")
    assert len(kdeconnect.submissions()) == 1, "opening Send to… must not send"
    kdeconnect.set({"phone1": {"name": "Pixel"}})
    strata.wait(lambda: "Tablet" not in strata.menu_items(), "disconnected target withdrawn")
    assert len(kdeconnect.submissions()) == 1
    strata.keyboard.press("Escape")
    strata.open_context_menu("todo.txt")
    strata.wait(lambda: "Send to…" in strata.menu_items(), "remaining device keeps Send to…")
    kdeconnect.set({})
    strata.wait(lambda: "Send to…" not in strata.menu_items(), "last device disconnected")
    assert {"Open", "Cut", "Copy"}.issubset(strata.menu_items())
    strata.dismiss_menu()


def test_keyboard_send_to_sends_whole_selection_to_chosen_device(strata, kdeconnect):
    kdeconnect.set({"phone1": {"name": "Pixel"}, "tablet2": {"name": "Tablet"},
                    "offline": {"name": "Offline", "reachable": False},
                    "unpaired": {"name": "Unpaired", "paired": False},
                    "disabled": {"name": "Disabled", "share": False}})
    other = 'second 冬 $HOME;`x` %23?.txt'
    strata.fixture.path(other).write_text("disposable transfer fixture")
    strata.wait(lambda: strata.entry(other), "second selected file")
    strata.select_entry("todo.txt")
    strata.click_entry_with(other, ["ctrl"])
    strata.open_context_menu(other)
    strata.wait(lambda: "Send to…" in strata.menu_items(), "eligible devices add Send to…")
    # Drive focus through the menu; no accessibility action substitutes for keys.
    strata.keyboard.press("Home")
    for _ in range(60):
        if strata.menu_item("Send to…").has_state("focused"):
            break
        strata.keyboard.press("Down")
    else:
        raise AssertionError("keyboard never reached Send to…")
    strata.keyboard.press("Right")
    strata.wait(lambda: "KDE Connect" in strata.menu_items(), "keyboard opens Send to…")
    for _ in range(8):
        if strata.menu_item("KDE Connect").has_state("focused"):
            break
        strata.keyboard.press("Down")
    else:
        raise AssertionError("keyboard never reached the KDE Connect submenu")
    strata.keyboard.press("Right")
    strata.wait(lambda: "Tablet" in strata.menu_items(), "keyboard opens the KDE Connect submenu")
    assert not {"Offline", "Unpaired", "Disabled"}.intersection(strata.menu_items())
    strata.keyboard.press("Home")
    for _ in range(8):
        if strata.menu_item("Tablet").has_state("focused"):
            break
        strata.keyboard.press("Down")
    else:
        raise AssertionError("keyboard never reached chosen device")
    strata.keyboard.press("Return")
    strata.wait_for_menu_closed()
    strata.wait(lambda: len(kdeconnect.submissions()) == 1, "batch submitted once")
    submission = kdeconnect.submissions()[0]
    assert submission["device"] == "tablet2"
    assert sorted(submission["uris"]) == sorted([strata.fixture.path(name).as_uri() for name in ("todo.txt", other)])
    strata.wait(lambda: strata.window.find(role="label", name="Files submitted to Tablet: 2. KDE Connect handles transfer progress and results."), "batch submission notice")
    assert strata.dialog() is None
