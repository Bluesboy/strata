# SPDX-License-Identifier: MIT
"""Session and per-scenario fixtures for the end-to-end suite."""

from __future__ import annotations

import functools
import json
import os
import re
import signal
import sys
import time
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent))

from harness import resources, screenshots, tree  # noqa: E402
from harness.application import Application, build_binary  # noqa: E402
from harness.artifacts import ArtifactCollector  # noqa: E402
from harness.browser import Strata  # noqa: E402
from harness.display import HeadlessDisplay  # noqa: E402
from harness.environment import TestEnvironment  # noqa: E402
from harness.fixtures import FixtureTree  # noqa: E402
from harness.interaction import Keyboard, Pointer  # noqa: E402
from harness.keyboard_layout import switched  # noqa: E402
from harness.xtest import INPUT_LOG_VARIABLE, XTestConnection  # noqa: E402

# Retry infrastructure startup only, never interaction assertions.
DISPLAY_START_ATTEMPTS = 2
# Set to a directory, each scenario writes its final window state, the
# `--qa` reply snapshot's shape, to `<directory>/<test id>.json`.
STATE_DUMP_VARIABLE = "STRATA_STATE_DUMP"
# How long the final state may keep changing before it is dumped anyway.
STATE_SETTLE_TIMEOUT = 5.0
STATE_SETTLE_INTERVAL = 0.25

pytest_plugins = ["harness.ci_plugin", "harness.warnings_plugin"]


def pytest_addoption(parser: pytest.Parser) -> None:
    parser.addoption(
        "--keep-artifacts",
        action="store_true",
        help="write the failure artifact bundle for passing scenarios too",
    )


def pytest_xdist_auto_num_workers(config: pytest.Config) -> int:
    cpus, memory = resources.available_resources()
    tasks = resources.available_tasks()
    try:
        workers = resources.worker_count(
            cpus,
            memory,
            os.environ.get("STRATA_E2E_WORKERS", "auto"),
            tasks,
        )
    except ValueError as error:
        raise pytest.UsageError(str(error)) from error
    task_text = f", {tasks} task slots" if tasks else ""
    print(
        f"E2E resources: {cpus:g} CPUs, {memory / resources.GIB:.1f} GiB available"
        f"{task_text}; {workers} workers"
    )
    return workers


@pytest.hookimpl(tryfirst=True)
def pytest_collection_modifyitems(config: pytest.Config, items: list[pytest.Item]) -> None:
    for item in items:
        if item.get_closest_marker("baseline"):
            item.add_marker(pytest.mark.xdist_group("visual-baselines"))


@pytest.fixture(scope="session")
def strata_binary() -> Path:
    return build_binary()


@pytest.fixture(scope="session")
def headless_display() -> HeadlessDisplay:
    last_error: Exception | None = None
    for _ in range(DISPLAY_START_ATTEMPTS):
        display = HeadlessDisplay()
        try:
            display.start()
        except Exception as error:
            last_error = error
            ArtifactCollector(test_name="infrastructure").write("startup-error.txt", str(error))
            display.stop()
            continue
        try:
            # libatspi reads these once, on the first `Atspi.init()`.
            os.environ.update(display.environment)
            tree.connect()
            yield display
        finally:
            display.stop()
        return
    raise AssertionError(f"the headless display could not be started: {last_error!r}")


@pytest.fixture(scope="session")
def xtest(headless_display: HeadlessDisplay) -> XTestConnection:
    connection = XTestConnection(headless_display.display)
    try:
        yield connection
    finally:
        tree.set_surface_origin_provider(None)
        connection.close()


@pytest.fixture
def keyboard(xtest: XTestConnection) -> Keyboard:
    return Keyboard(xtest)


@pytest.fixture
def pointer(xtest: XTestConnection) -> Pointer:
    return Pointer(xtest)


@pytest.fixture
def keyboard_layout(xtest: XTestConnection):
    """Switch the private display's layout live: `with keyboard_layout(RUSSIAN): ...`."""

    return functools.partial(switched, xtest)


@pytest.fixture
def fixture_tree() -> FixtureTree:
    """A fresh fixture tree per scenario."""

    tree_ = FixtureTree.create()
    try:
        yield tree_
    finally:
        tree_.cleanup()


@pytest.fixture
def test_environment() -> TestEnvironment:
    environment = TestEnvironment()
    try:
        yield environment
    finally:
        environment.cleanup()


@pytest.fixture
def preferences() -> dict[str, object]:
    """Overrides a scenario applies before the application starts.

    Parametrise it with `@pytest.mark.preferences(browser_mode="icons")`.
    """

    return {}


@pytest.fixture
def strata(
    request: pytest.FixtureRequest,
    strata_binary: Path,
    headless_display: HeadlessDisplay,
    test_environment: TestEnvironment,
    fixture_tree: FixtureTree,
    keyboard: Keyboard,
    pointer: Pointer,
) -> Strata:
    overrides: dict[str, object] = dict(request.getfixturevalue("preferences"))
    for marker in request.node.iter_markers("preferences"):
        overrides.update(marker.kwargs)
    test_environment.write_preferences(overrides)

    application = Application(
        display=headless_display,
        environment=test_environment,
        location=fixture_tree.root,
    )
    window = Strata(
        application=application,
        keyboard=keyboard,
        pointer=pointer,
        fixture=fixture_tree,
        environment=test_environment,
        display=headless_display,
    )
    failed = False
    try:
        application.start()
        _start_input_log(request, window)
        yield window
    except BaseException:
        failed = True
        raise
    finally:
        try:
            window.keyboard.connection.stop_input_log()
            _dump_state(request, window)
            if failed or request.node.stash.get(_REPORT_KEY, False) or request.config.getoption(
                "--keep-artifacts"
            ):
                _collect_artifacts(request, window)
        finally:
            application.stop()


def node_file_name(node_id: str) -> str:
    """The file a scenario's input log and state dump are named after.

    `scripts/qa_parity.py` derives the same name from a twin's node id.
    """

    return re.sub(r"[^A-Za-z0-9._-]+", "_", node_id)


def _start_input_log(request: pytest.FixtureRequest, window: Strata) -> None:
    directory = os.environ.get(INPUT_LOG_VARIABLE)
    if not directory:
        return

    def origin() -> tuple[int, int]:
        bounds = window.window.screen_bounds()
        return bounds.x, bounds.y

    window.keyboard.connection.start_input_log(
        Path(directory) / f"{node_file_name(request.node.nodeid)}.log", origin
    )


def _dump_state(request: pytest.FixtureRequest, window: Strata) -> None:
    directory = os.environ.get(STATE_DUMP_VARIABLE)
    if not directory:
        return
    try:
        state = _settled_snapshot(window)
    except Exception as error:  # the window may already be gone after a failure
        state = {"error": repr(error)}
    path = Path(directory) / f"{node_file_name(request.node.nodeid)}.json"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps({"nodeid": request.node.nodeid, "app": state}, indent=2) + "\n")


@pytest.fixture
def unreserved_columns(strata: Strata) -> None:
    """Explicitly reclaim preview space for workflows needing visible parent panes."""
    strata.open_appearance_menu()
    option = strata.wait(
        lambda: strata.window.find(role="toggle button", name="Preview panel"),
        "the preview panel toggle",
    )
    if option.has_state("pressed"):
        strata.pointer.click(option)
        strata.wait_for_menu_closed()
    else:
        strata.dismiss_menu()
    strata.keyboard.press("ctrl+l")
    strata.wait(
        lambda: strata.window.find(role="text", states={"editable", "focused"}),
        "the location editor to take focus",
    )
    strata.keyboard.press("Escape")
    strata.wait(lambda: strata.focused_pane(), "focus to return to the file listing")


def _settled_snapshot(window: Strata) -> dict[str, object]:
    """The snapshot once two reads in a row agree, as the `--qa` runner takes it."""

    deadline = time.monotonic() + STATE_SETTLE_TIMEOUT
    previous = None
    while True:
        state = window.state_snapshot()
        if state == previous or time.monotonic() >= deadline:
            return state
        previous = state
        time.sleep(STATE_SETTLE_INTERVAL)


_REPORT_KEY = pytest.StashKey[bool]()


def _collect_artifacts(request: pytest.FixtureRequest, window: Strata) -> None:
    collector = ArtifactCollector(test_name=request.node.name)
    collector.collect(
        display=window.display.display,
        accessibility_tree=window.application.diagnostics(),
        application_log=window.application.log(),
        fixture_listing=window.fixture.listing(),
        session_logs=window.display.logs(),
    )
    print(f"\nfailure artifacts: {collector.directory}")


_TERMINATION_HANDLER_KEY = pytest.StashKey[object]()


def _terminate_session(_number, _frame):
    raise KeyboardInterrupt("headless test session terminated")


def pytest_unconfigure(config: pytest.Config) -> None:
    previous = config.stash.get(_TERMINATION_HANDLER_KEY, None)
    if previous is not None:
        signal.signal(signal.SIGTERM, previous)


def pytest_configure(config: pytest.Config) -> None:
    if config.getoption("numprocesses", 0) and config.getoption("dist", "no") != "loadgroup":
        raise pytest.UsageError("Parallel E2E requires --dist=loadgroup to isolate visual baselines")
    config.stash[_TERMINATION_HANDLER_KEY] = signal.signal(signal.SIGTERM, _terminate_session)
    config.addinivalue_line(
        "markers", "preferences(**values): seed Strata preferences for a scenario"
    )
    config.addinivalue_line(
        "markers", "baseline: a scenario that compares against a golden screenshot"
    )


@pytest.hookimpl(hookwrapper=True)
def pytest_runtest_makereport(item: pytest.Item, call: pytest.CallInfo):
    outcome = yield
    report = outcome.get_result()
    if report.when == "call" and report.failed:
        item.stash[_REPORT_KEY] = True


@pytest.fixture
def baseline(request: pytest.FixtureRequest):
    """Compare a capture with its committed golden image."""

    def compare(window: Strata, name: str) -> screenshots.Comparison:
        collector = ArtifactCollector(test_name=request.node.name)
        actual = window.screenshot(collector.directory / f"{name}.capture.png")
        comparison = screenshots.compare_to_baseline(name, actual, collector.directory)
        assert comparison.matched, (
            comparison.summary
            + f"\nExpected, actual, and diff images are in {collector.directory}"
            + "\nRegenerate deliberately with STRATA_E2E_UPDATE_BASELINES=1."
        )
        return comparison

    return compare
