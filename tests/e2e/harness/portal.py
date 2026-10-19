# SPDX-License-Identifier: MIT
"""Portal file-chooser requests made against Strata's backend on the private bus."""

from __future__ import annotations

from concurrent.futures import Future, ThreadPoolExecutor
from contextlib import contextmanager
from pathlib import Path
from types import SimpleNamespace
from typing import Iterator
import uuid

from gi.repository import Gio, GLib

from . import parity, tree
from .browser import Strata
from .display import HeadlessDisplay
from .environment import TestEnvironment, process_environment
from .fixtures import FixtureTree
from .process import ManagedProcess, terminate
from .tree import Node

BACKEND = "org.freedesktop.impl.portal.desktop.strata"
DESKTOP = "/org/freedesktop/portal/desktop"
APPLICATION = "io.github.lgse.Strata.FileChooser"


@contextmanager
def open_file_request(
    strata_binary: Path,
    headless_display: HeadlessDisplay,
    test_environment: TestEnvironment,
    fixture_tree: FixtureTree,
    *,
    title: str,
    options: dict[str, GLib.Variant],
    browser_mode: str,
) -> Iterator[tuple[Node, Future]]:
    """Start the backend, send `OpenFile`, and yield the chooser and its pending response.

    The request is closed on exit if the scenario ended before it was answered.
    """

    test_environment.write_preferences({"browser_mode": browser_mode})
    environment = {**process_environment(), **test_environment.variables(), **headless_display.environment}
    backend = ManagedProcess.spawn("portal", [str(strata_binary), "--portal"], log_dir=test_environment.root, env=environment, cwd=fixture_tree.root)
    connection = Gio.DBusConnection.new_for_address_sync(
        environment["DBUS_SESSION_BUS_ADDRESS"],
        Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION,
        None, None,
    )
    pool = ThreadPoolExecutor(max_workers=1)
    handle = f"{DESKTOP}/request/strata_test/request_{uuid.uuid4().hex}"
    future = None
    chooser = None
    try:
        tree.wait_until(lambda: connection.call_sync(
            "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "NameHasOwner",
            GLib.Variant("(s)", (BACKEND,)), None, Gio.DBusCallFlags.NONE, 1000, None,
        ).unpack()[0], message="portal backend registration", timeout=20)
        parameters = GLib.Variant("(osssa{sv})", (handle, "", "", title, options))
        future = pool.submit(connection.call_sync, BACKEND, DESKTOP, "org.freedesktop.impl.portal.FileChooser", "OpenFile", parameters, GLib.VariantType.new("(ua{sv})"), Gio.DBusCallFlags.NONE, 60000, None)

        def window():
            application = tree.find_application(APPLICATION)
            return application.find(name=title) if application else None

        chooser = tree.wait_until(window, message=f"{title} chooser", timeout=20)
        _start_parity_log(headless_display, chooser)
        yield chooser, future
    finally:
        _end_parity_log(headless_display, chooser, backend)
        if future is not None and not future.done():
            try:
                connection.call_sync(BACKEND, handle, "org.freedesktop.impl.portal.Request", "Close", None, None, Gio.DBusCallFlags.NONE, 2000, None)
            except GLib.Error:
                pass
        terminate(backend.popen)
        pool.shutdown(wait=True, cancel_futures=True)
        connection.close_sync(None)


def _start_parity_log(display: HeadlessDisplay, chooser: Node) -> None:
    """Logs input relative to the chooser, as the `--qa` twin's window is it."""

    node_id = parity.current_node_id()
    if display.input_connection is None or node_id is None:
        return

    def origin() -> tuple[int, int]:
        bounds = chooser.screen_bounds()
        return bounds.x, bounds.y

    parity.start_input_log(display.input_connection, node_id, origin)


def _end_parity_log(display: HeadlessDisplay, chooser: Node | None, backend: ManagedProcess) -> None:
    """Closes the input log and dumps the chooser's state before the request ends."""

    node_id = parity.current_node_id()
    if display.input_connection is not None:
        display.input_connection.stop_input_log()
    if chooser is None or node_id is None:
        return
    # `Strata.state_snapshot` reads only `self.window`, here the chooser.
    view = object.__new__(Strata)
    view.application = SimpleNamespace(root=chooser)
    parity.dump_state(node_id, view.state_snapshot, backend.exited)
