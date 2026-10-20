# End-to-end GUI testing

The Rust suite covers state transitions, filesystem edge cases, and individual
widgets. The end-to-end suite in `tests/e2e` drives the real GTK
application on a headless X display, sends real keyboard and pointer input, and
checks both what the window reports and what happened on disk.

## Running it

```bash
./scripts/e2e.sh                     # every scenario
./scripts/e2e.sh -k drag             # one selection
./scripts/e2e.sh -k "clipboard and columns"
```

### Targeted local validation

Choose checks from the behavior and caller mapping, not from changed-file names
alone. Bounded changes may use a non-empty, relevant selection; broad,
cross-cutting, shared infrastructure, dependency/build/CI/harness changes, or
uncertain coverage require the full pinned quality phases and the full canonical
E2E run. Include relevant views, callers, and preference behavior, and add or
run the regression test that proves the change. Record the scope rationale,
commands, results, and intentional omissions in the handoff. After editing,
rerun affected tests rather than unrelated suites.

Run local lint and formatting checks only before pushing, not during the
edit/test loop. For Rust changes, run `./scripts/quality.sh fmt` and
`./scripts/quality.sh clippy` on the final code at that checkpoint; rerun affected
checks if fixes change the code before the push. Even when full Rust tests are
needed during iteration, use `./scripts/quality.sh test` and defer lint/format
phases until pre-push. CI's lint and formatting checks remain unchanged.

The repository owner may explicitly authorize pushing a PR without the normally
required local tests for the current change/push. A general push request,
urgency, silence, or consent for another push does not qualify. Record the
explicit authorization, omitted suites, and known failures or unverified behavior
in the handoff and PR description; never report skipped tests as passing. This
exception does not waive lint/format checks, GUI isolation, or required CI/merge
checks. Renew consent if the authorized scope changes, or run the required tests.

For native Rust selection, `scripts/test-headless.py` starts the private display
and session buses, then appends its arguments to the fixed
`cargo test --all-targets --all-features` command. Use module/name filters:

```bash
./scripts/test-headless.py services::operations::tests
./scripts/test-headless.py basenames_reject_empty_reserved_nested_absolute_and_nul_names
```

These select test execution across targets, not compilation to one target.
Targeted runs can still incur a full test build; Cargo's cache helps subsequent
iterations. There is no automatic changed-code dependency-to-test mapping.

Controller regressions live under `src/app/browser/tests/`, grouped by navigation,
location input, selection, monitor updates, preferences, operations, undo, metadata,
staging, event dispatch, and archive activation. Add cases to the matching owner
rather than to `src/app/browser/tests.rs`, which holds shared source/provider
fixtures. Existing adjacent feature test modules retain their ownership. For
example, `./scripts/test-headless.py app::browser::tests::undo::` selects undo
history/claim regressions; `app::browser::tests::metadata::` selects metadata-fill
and sort-terminal regressions. Keep distinct input routes and lifecycle cases
separate even when they share fixtures.

Local-operation regressions have owners under `src/adapters/local_operations/tests/`
for path resolution, copying, moving, replacement, conflicts, progress, naming,
paste results, undo, deletion, and restore safety. Shared fixtures and the manual
deletion benchmarks remain in the parent `tests.rs`; keep the benchmark names
stable for `scripts/benchmark-delete.sh`. For example,
`./scripts/test-headless.py adapters::local_operations::tests::replacement::`
selects replacement safety cases. Preserve independently named filesystem races,
symlink, non-UTF8, cancellation, and conflict scenarios when sharing setup.

Window regressions have policy owners under `src/ui/window/tests/` for keyboard
classification, sidebar places, bookmarks, devices, and trash. Startup and live
preference cases stay in `preferences`; real key-routing scenarios stay in
`keyboard_dispatch` and `type_to_search`. The `ui::window::` filter covers these
owners and the adjacent window modules. When moving a GTK case, update its
`gtk_test` subprocess name to match its compiled test path and verify that the
child actually executes the case; an unchanged outer test count is not enough.

A filter must collect at least one test; use Cargo's output or a collection
check to verify that it did. `scripts/quality.sh` only accepts `all`, `fmt`,
`clippy`, or `test` and does **not** forward test filters, so it cannot be used
for a targeted test selection.

The E2E runner passes repository-relative paths and normal pytest arguments to
its configured pytest invocation. Use a file or `-k` expression, then confirm
collection is nonzero:

```bash
./scripts/e2e.sh tests/e2e/scenarios/test_inline_renaming.py
./scripts/e2e.sh -k 'rename and not visual' --collect-only
```

`./scripts/e2e.sh` is the canonical pinned-container evidence; native
`scripts/e2e-native.sh` is only for host-toolkit debugging and does not replace
it. Preserve the verified image provenance and `target/e2e-container` and
`target/quality-container` caches. CI still runs its complete unchanged gate.

Every GUI or delegated check must clear inherited display variables and use a
private Xvfb and private D-Bus session. `scripts/test-headless.py` and the E2E
runners enforce this isolation. Rust tests set `GTK_A11Y=none`, `NO_AT_BRIDGE=1`,
and `STRATA_REQUIRE_GTK_TESTS=1`; E2E enables accessibility on its private AT-SPI
bus to drive the application. Other commands must arrange equivalent isolation. A
missing isolated display or bus is a hard failure. Never run against the desktop
or an inherited session bus, silently skip GTK tests, or fall back to the desktop.

The runner prefers Podman when available; select an engine explicitly with
`STRATA_CONTAINER_ENGINE=podman` or `docker`. CI explicitly selects Docker to match
its runtime archive loader; an image in one engine's store is not visible to the
other. Normal runs verify and reuse the
local base image. If missing, they pull the published environment once, verify
its input label and platform, and record it locally. They **never automatically
build images or fetch distribution packages**. A missing publication fails with
an actionable message rather than silently bootstrapping.

When intentionally updating the environment, or before its first publication,
explicitly build the local base once:

```bash
STRATA_CONTAINER_ENGINE=podman python3 scripts/e2e_base.py build
./scripts/e2e.sh
```

The recipe is `tests/e2e/Dockerfile`: a digest-pinned Arch Linux base, the dated
2026-09-11 Arch package snapshot, GTK 4.22.5, GLib 2.88.3, GStreamer 1.28.7,
Rust 1.98.1, and pinned Python dependencies. Pacman verifies the snapshot's
signed packages. The image replaces only GTK's versioned shared library with a
matching-source build carrying the accessibility-coordinate patch documented in
[`tests/e2e/gtk/README.md`](../tests/e2e/gtk/README.md); an image-build probe checks
the loaded library and rendered/accessibility coordinates on a private display.
Changing those inputs selects a new base; application edits do not. The normal
runner still compiles the checked-out application and retains Cargo's worktree
cache. No host GTK libraries, fonts, desktop sockets, or Rust binaries are mounted.

The checkout is mounted at `/workspace`; pass test paths relative to the
repository. Build and Cargo caches live in `target/e2e-container`, separately
from native builds. Artifacts remain in `target/e2e-artifacts` and are owned by
the invoking user. Minimal generated passwd/group files provide the invoking
UID/GID to D-Bus, so one published environment works across local user IDs without
rebuilding it or mounting the host's account database.
The image includes bubblewrap for sandboxed thumbnail decoding, FFmpeg/ffprobe,
and GTK's GStreamer media backend with the base/good/libav plugins. Rust media
regressions exercise actual normalization, playback, and long-source duration
limits rather than skipping when optional host tools are missing. These packages
come from the dated Arch snapshot. The accessibility patch is test-only and does
not apply or retire the version-specific GTK/GstPlay work in
`packaging/media-runtime/`; that opt-in kit remains unchanged and is not shipped
by this image update.

Rootless Podman
runs unmask `/proc/*` inside the test container so bubblewrap can mount its own
private `/proc`; the decoder's sandbox and the container's seccomp policy remain
enabled. Docker's outer seccomp/AppArmor profiles and system-path masks must be
disabled for the nested namespace and mount operations. This applies only to the
disposable E2E container; Strata still launches its normal bubblewrap decoder
sandbox. Neither engine uses privileged mode or mounts desktop sockets.
Updating the image inputs is an intentional rendering
environment change and requires reviewing the visual baselines.

For explicit host-toolkit debugging only, `./scripts/e2e-native.sh` accepts
`STRATA_BINARY` and `STRATA_E2E_VENV`. A native pass does not replace canonical
`./scripts/e2e.sh` evidence when targeted or full E2E validation is required.

### Shared environment for formatting, lint, and Rust tests

`./scripts/quality.sh` runs all three quality phases in the same verified build
base. Use `./scripts/quality.sh fmt`, `clippy`, or `test` for an individual phase.
No separate quality image is necessary: the published environment already has
Rust 1.98.1, rustfmt, Clippy, native development libraries, and Xvfb. Quality now
uses that pinned compiler rather than moving `stable`.

CI resolves the public environment to a manifest digest, checks its input labels
and platform, and pulls that exact digest without registry credentials. Each
phase verifies the loaded image again and runs its immutable image ID. An
unavailable unchanged environment fails explicitly instead of installing packages.
For a deliberate recipe-input change relative to the PR base (or previous main
commit), CI may explicitly build that unpublished candidate from the pinned
recipe, without publishing it. This lets environment-update PRs pass before the
trusted-main publisher runs; it does not turn registry outages into repeated
distribution bootstraps. Locally, environment builds still require the explicit
command shown above.

Quality's Cargo home and build directory are under `target/quality-container`,
separate from E2E and native builds. Its Actions cache is keyed by environment,
Cargo manifests, and source revision, with same-environment/manifest restoration.
Only successful quality builds on main pushes save caches; PR consumers cannot
populate main's cache. These are compilation caches, not evidence of passing tests;
the aggregate gate separately requires all test shards to pass. Cold quality
compilation includes test-only and all-feature dependencies, so the E2E application
dependency cache is not advertised as a full quality hit.

The Rust suite runs with `--all-targets --all-features --locked` inside private
Xvfb, with `GTK_A11Y=none`, `NO_AT_BRIDGE=1`, and `STRATA_REQUIRE_GTK_TESTS=1`.
The shared Xvfb server uses `-noreset` so consecutive test processes do not
race a server reset when the last display client exits. GTK initialization
failures cannot silently skip tests. The container's `/tmp`, which holds `HOME` and
the tests' temporary folders, is an executable tmpfs: copy tests need `O_TMPFILE`
there, which an overlay `/tmp` may refuse. Formatting, compiler,
lint, and test failures remain blocking. Lightweight policy/helper jobs retain
their existing runners rather than downloading a large GUI image unnecessarily.

### Rust quality shards

CI's **Quality build and lint** job runs formatting and Clippy, then compiles
`cargo test --locked --all-targets --all-features --no-run` once. It exports only
test executables and a plan, not Cargo caches. `scripts/quality_ci.py` collects
each libtest inventory, including the explicitly ignored tests, and assigns every
entry to one of four shards. Timing hints in `scripts/quality-durations.json`
come from successful GTK child runs in
[run 34560353003](https://github.com/lgse/strata/actions/runs/34560353003).
Tests are balanced longest-first across the shards; unknown tests receive a
one-second weight and always participate. Timing hints are not an allowlist.
The deferred-scroll timing was refreshed to 1.32 seconds from
[run 34811363582](https://github.com/lgse/strata/actions/runs/34811363582).
`ui::search::tests::deferred_scroll_restoration_yields_to_updates_wheel_scrollbar_and_query_reset`
runs in its own libtest process before the other tests assigned to its shard,
not on a dedicated runner. Each process must pass its complete selection before
an aggregate shard receipt is written. Validation rejects a missing, duplicated,
or ignored isolated test; renaming or removing it requires updating `ISOLATED_TEST`.
Sharding does not shorten an individual test.

Each **Rust tests shard N** verifies the checkout revision, application/Rust-test source
fingerprint, image inputs, executable checksums, and the entire libtest inventory
before selecting exact names. Tests share neither a display nor a session bus
with another shard. The existing per-process GTK serialization still applies.
The runner checks libtest's actual selection and final passed/ignored counts
before writing a success receipt; nonzero exits, empty runnable shards, or changed
inventories fail. Existing `#[ignore]` entries stay explicitly accounted for;
sharding neither enables them nor silently ignores additional tests. Unsupported
non-libtest harness inventories fail closed rather than disappearing from coverage.
The QA runner, `tests/qa.rs`, has its own harness (`harness = false`, see
"Scheduling and run costs") that keeps libtest's `--list`, `--ignored`,
`--exact`, and `test result:` formats, so its scenario runs are inventoried as
explicitly ignored tests and its unit tests run in the shards like any other.

Published environments are pulled by the build job's exact manifest digest and
verified again by `quality.sh`. Deliberate unpublished recipe updates transfer
the same locally built environment as an attempt-scoped artifact instead; shards
never rebuild it. **Format, lint, and test** retains the required-check name,
requires successful build/lint and every shard, and verifies no missing, extra,
duplicate, or stale receipts. Failed shards are not retried, and `fail-fast: false`
preserves the other shards' results. Reports and logs are attempt-scoped artifacts.

To reproduce the handoff locally using the same pinned container:

```bash
STRATA_QUALITY_TASK=build ./scripts/quality.sh test
for shard in 0 1 2 3; do
  STRATA_QUALITY_TASK=shard STRATA_QUALITY_SHARD="$shard" ./scripts/quality.sh test
done
python3 scripts/quality_ci.py verify
```

The example runs shards sequentially for convenient local diagnosis; CI runs them
in parallel on separate runners. These environment variables are CI handoff modes,
not test filters; ordinary `./scripts/quality.sh test` still runs the complete
unsharded suite. Rebuild the bundle after changing source or checkout revision.
The two-shard matrix and `SHARDS` constant must be updated together if tuning
fan-out. Each shard has a ten-minute hang bound; timing is otherwise informational.

### Hardware-aware parallelism

The canonical container and native debugging runner default to isolated
`pytest-xdist` workers. Each worker owns its Xvfb server, private session and
accessibility buses, and input connection; scenarios within a worker run
serially. The application is built once before workers start.

Auto mode chooses the smallest of:

- half the available logical CPUs (rounded down), respecting CPU affinity and
  cgroup v1/v2 quotas, including visible ancestor limits;
- one worker per 2 GiB of available memory after reserving 1 GiB, respecting
  host `MemAvailable` and remaining cgroup memory;
- one worker per 256 available cgroup task slots after reserving 128 slots for
  the controller, X servers, buses, and toolkit/media threads;
- 16 workers.

At least one worker runs, including when memory or PID availability is unknown.
The budget is detected **inside the container, after compilation**, and printed at
startup. It is a conservative resource budget, not a promise of linear speedup.

```bash
STRATA_E2E_WORKERS=auto ./scripts/e2e.sh  # default, locally and in CI
STRATA_E2E_WORKERS=8 ./scripts/e2e.sh     # explicit budget
STRATA_E2E_WORKERS=1 ./scripts/e2e.sh     # serial scenarios
./scripts/e2e.sh -n 0                   # no worker subprocess, for debugging
```

A positive override bypasses the automatic resource caps. Explicit numeric
pytest `-n` options take precedence over the environment variable. Parallel runs require
`--dist=loadgroup`: all visual-baseline scenarios share one scheduling group so
their fixed fixture directory is never claimed concurrently. Other scenarios
are distributed individually. Worker crashes fail the run without automatic
replacement or assertion retries. Baseline updates use the same grouping.
Compare worker budgets with a warm build cache; xdist does not speed up image
setup or Rust compilation.

### Keep Rust test windows off the local desktop

Some Rust tests also create GTK windows when a display is available. Run the
complete suite, or a justified targeted selection, on the same private
Xvfb/private D-Bus infrastructure with:

```bash
./scripts/test-headless.py
./scripts/test-headless.py -- --nocapture
./scripts/test-headless.py relevant_module_or_test_name
```

This requires Xvfb and AT-SPI but not the Python E2E packages. It isolates
application preferences while retaining access to the installed Cargo/Rust
toolchains, and disables accessibility bridging for Rust tests. A startup failure
aborts; it never falls back to the real display or an inherited session bus.
The E2E runner likewise clears inherited display variables before startup.

### Native debugging dependencies

These are installed inside the canonical image; only native debugging needs
them on the host.

| Purpose | Arch | Debian / Ubuntu |
| --- | --- | --- |
| Headless X server | `xorg-server-xvfb` | `xvfb` |
| Accessibility bus and registry | `at-spi2-core` | `at-spi2-core` |
| Python AT-SPI bindings | `python-gobject` | `python3-gi`, `gir1.2-atspi-2.0` |
| Private session bus | `dbus` | `dbus-daemon`, `dbus-bin` |
| Keyboard layouts | `xorg-setxkbmap` | `x11-xkb-utils` |
| Screen capture | `imagemagick` | `imagemagick` |
| Font | `cantarell-fonts` | `fonts-cantarell` |

`pytest`, `pytest-xdist`, and Pillow are installed into the virtual environment from
`tests/e2e/requirements.txt`. The environment is created with
`--system-site-packages` because PyGObject is a system package.

## How a scenario runs

Each scenario gets:

- a fresh fixture tree in its own temporary directory, with fixed modification
  times, materialised by `harness/fixtures.py` from the shared manifest
  `tests/fixtures/harness.json`, which also defines the seeded preferences,
  GTK settings, static environment, and keyboard layouts;
- a throwaway `HOME` and XDG directories, seeded with a complete
  `settings.toml` so no preference is inherited from a default change
  (`harness/environment.py`);
- a private D-Bus session with no service directories, so nothing is
  D-Bus-activated behind the suite's back — no desktop portal, no document
  portal FUSE mount;
- an accessibility bus and registry started by the harness rather than by
  systemd activation.

Rendering is pinned: Xvfb at 1440x900x24 and 96 DPI, `GSK_RENDERER=cairo`,
software GL, `GDK_SCALE=1`, the Adwaita icon theme, Cantarell 11,
`C.UTF-8`, `UTC`, animations off, and `reduce_motion` on.

Application preferences and file operations use only the scenario's temporary
HOME/XDG directories and fixtures. Both are created under `/tmp` so trash
capabilities do not vary when the caller's `TMPDIR` is on another filesystem. System libraries, fonts, and icon assets are
provided by the container; desktop endpoints and user configuration overrides
are not inherited.

## Writing a scenario

Scenarios talk to `harness.browser.Strata`, which locates controls by
accessible role, name, and state:

```python
def test_cut_moves_only_after_paste(strata):
    fixture = strata.fixture

    strata.select_entry("todo.txt")
    strata.keyboard.press("ctrl+x")
    assert fixture.path("todo.txt").exists()

    strata.open_directory("archive")
    strata.paste_into("archive")

    strata.wait(
        lambda: fixture.path("archive/todo.txt").exists(),
        "the cut file to arrive in archive",
    )
```

Rules the suite holds itself to:

- **Locate by accessibility, never by pixels.** Pointer targets are derived
  from a located node's accessible bounds. A literal screen coordinate in a
  scenario is a defect.
- **Synchronize on conditions, never on sleeps.** `strata.wait(...)` polls a
  predicate and, on timeout, prints the accessibility tree. There are no
  `time.sleep` calls in the scenarios; the small gaps inside
  `harness/interaction.py` are transport settling between synthetic X events,
  not waits on application state.
- **Assert on the filesystem as well as the window.** Every file operation
  checks the resulting tree, not only the listing.
- **Read chosen states from AT-SPI.** Check, radio and toggle controls expose
  `checked` or `pressed` (`node.has_state("checked")`); never infer them from
  check-icon children.
- **Run one scenario per presentation where it matters.** `harness.modes`
  supplies the `ALL_MODES` parameterization.

To exercise a preference, mark the scenario:

```python
@pytest.mark.preferences(browser_mode="icons", type_to_search=False)
def test_something(strata):
    ...
```

Columns reserves preview space by default, including while preview content is
closed. Scenarios whose contract needs several parent columns visible at once
may use `@pytest.mark.usefixtures("unreserved_columns")`. This fixture switches
**Appearance → Preview panel** off through the UI and returns keyboard focus to
the listing; it does not change saved preferences. Do not apply it to preview
lifecycle scenarios or use it as a suite-wide default. Their reserved-space
behavior and the default overflow baseline must remain covered. The layout
rules those scenarios own are listed in
[Preview panel and column layout](preview-panel-layout.md).

### Inline new-entry focus regressions

`test_inline_renaming.py` checks immediate default-file/folder creation, collision
numbering, selected default names, valid-name commits on click-away, and retaining
the original name on Escape or representative invalid input. Enter and sidebar
commits still run existing and newly created files and folders in all three views;
the four shared click-away targets run in Columns and List on an existing file.
Invalid names keep one Enter and one real click-away; correction/reopen still runs in all three views.
Escape cancellation is kind × new in the default Columns view. Repeated renames
keep file-stem vs folder-name selection in Columns and List. `test_entry_management.py`
also covers reopening invalid edits, inside-field clicks, name conflicts, and empty
directories.

```bash
./scripts/e2e.sh tests/e2e/scenarios/test_inline_renaming.py tests/e2e/scenarios/test_entry_management.py
```

The long-name scenario sends real F2, End, Left/Right, typing, Backspace, and
inside-field clicks. It uses a 420×300 Columns window with the sidebar open,
so the column is wider than the browser viewport; List/Icons use 640×300 to
leave room for their minimum card/table widths. The adjacent Rust caret fixture
requires mapped, non-zero-width editors and overflowing text, then checks the
scroll-adjusted caret against both `GtkText` and browser bounds, including
viewport resizes. `./scripts/e2e-mutation-check.sh rename-caret` proves that an
editor sized to the whole name instead of the viewport is detected.

Keep these as real XTEST pointer interactions: emitting a focus controller's
`leave` signal in a Rust test checks the handler, not GTK's in-flight focus walk
(#566). Rename dispatch must wait until that walk returns because an operation
can refresh the row model. Creation now uses real entries rather than temporary
placeholder rows; the same rename path handles new and existing items. Rust
tests cover atomic naming collisions, Unicode validation, editor lifetimes,
cancellation/navigation before the created entry becomes visible, and scrolling
to new entries beyond the initial viewport in large directories.

### Accessible names are product surface

The harness finds an entry because Strata names it. Those names live in
`src/ui/accessibility.rs` and exist for screen readers first: an entry row is
labeled with its name and described as `Folder` or `File`, a pane is labeled
with its directory and described with its presentation, the focusable surface
of an empty, unreadable or loading pane is labeled with its directory and
described with its status text, menu items carry the
menu role with the accelerator in the description, and modal dialogs carry the
dialog role and a name. When the harness cannot identify a control, the fix is
to name it in the application, not to reach around the accessibility layer.

### Input

Keyboard and pointer events go through XTEST (`harness/xtest.py`). AT-SPI's own
`GenerateMouseEvent` never replies on a headless server, so the harness talks to
the same X extension `at-spi2-registryd` would have used. Discovery and state
inspection still go through AT-SPI. The harness can resolve a popup's native
surface origin through X11 using its accessible dimensions; GTK 4.22 normally
reports that native origin directly. Controls are still located only by
accessibility semantics. AT-SPI's older `push button` spelling is normalized to
`button`, and selection tests assert actual selected-state transitions instead
of depending on optional `SELECTABLE` state exports for unselected rows.

#### Keyboard layouts

The `keyboard_layout` fixture switches the layout live, while the application
runs, using the layouts in `harness/keyboard_layout.py`:

```python
from harness.keyboard_layout import RUSSIAN

def test_search(strata, keyboard_layout):
    with keyboard_layout(RUSSIAN):
        strata.keyboard.press("ctrl+k")
        strata.editable_field()
```

`setxkbmap -display` loads the layouts, and their XKB variants when a `Layout`
names them, into the private Xvfb, and an XKB group lock selects the active
one, as a layout switch shortcut does. Leaving the block unlocks the group and
every modifier, restores the US layout without variants, and fails the
scenario if the keyboard is not back in that state. Never change the desktop's
layout, and never run `setxkbmap` without `-display`.

XTest reads the keymap once, from the first group of the default US layout, so
key names mean US key positions: under `RUSSIAN`, `press("ctrl+c")` presses the
key that types с, and `type_text` types what those keys produce. The `altgr`
modifier presses the US right Alt key, which layouts with AltGr turn into it:
under `PINYIN`, `press("altgr+1")` types the macron dead key. Wait for each
key's effect inside the block. GDK may read the keymap only when it handles the
event, and by then a finished block has restored US.

The harness runs no input method daemon. GTK's built-in input method composes
dead keys in text fields, so a pending `PINYIN` tone mark stands in for an input
method's unfinished composition when a scenario checks which keys reach it.

## Failure artifacts

A failing scenario writes a directory under `target/e2e-artifacts/<worker>/<test name>`
(the worker component is omitted with `-n 0`)
containing a screenshot, the accessibility tree, the application log, the
fixture tree listing, and the Xvfb, D-Bus, and AT-SPI logs. The path is printed
in the test output, and CI uploads the whole directory. Pass
`--keep-artifacts` to collect them for passing scenarios too.

## Visual baselines

`tests/e2e/scenarios/test_visual_baselines.py` compares a small set of stable
states with the images in `tests/e2e/baselines/gtk-4.22`: one canonical fixture
in each view, a hovered Icons tile, a selection with focus, an open context menu,
and a confirmation dialog. Local and CI runs use this one rendering profile. Other host GTK
versions do not have separate baselines; native baseline runs fail rather than
silently accepting a different renderer.
These scenarios exclusively claim `/tmp/strata-e2e-baseline`, because the
breadcrumb and context menu render the full path. An existing directory or
symlink is a setup error, never deleted or reused. Within a run, baseline
scenarios stay on one worker; independent concurrent runs must use separate
containers. A native host run interrupted before its cleanup can leave
`/tmp/strata-e2e-baseline` behind; remove it by hand before running them again.

A capture matches when no more than 0.5% of pixels differ by more than 24 in
any channel, which absorbs the subpixel antialiasing that software rendering
varies between runs.

Baselines are never accepted automatically. When a change is intended:

```bash
STRATA_E2E_UPDATE_BASELINES=1 ./scripts/e2e.sh -k baseline
```

Then review the new images and commit them with the change, so the difference
is visible in the pull request. The QA twins in
`tests/qa/scenarios/test_visual_baselines/` compare their captures with the
same images and tolerances through a `baseline` step and never write them, so
regenerated images serve both suites. On a mismatch the suite writes the expected,
actual, and diff images into the artifact directory.

## Proving the suite would catch a regression

`scripts/e2e-mutation-check.sh` applies one deliberate defect at a time from
`tests/e2e/mutations`, rebuilds, and asserts that the scenarios for that
workflow fail:

```bash
./scripts/e2e-mutation-check.sh              # every mutation
./scripts/e2e-mutation-check.sh clipboard    # one of them
```

Each patch breaks a single critical workflow and maps to the scenario that
hits the mutated line, not every test in that file: drop onto a folder,
Ctrl+V paste, arrow-key selection, single-click activation, the appearance
menu, filtered Space preview, filter-row updates, popover outside-wheel,
the long-name editor staying in the viewport, a live click-mode preference,
cut deferring its move to the paste, Return in the location editor, Keep Both
selecting the numbered copy, Enter, click-away and Escape in the inline
editor, a new entry colliding with an existing name, the rename error dialog,
Delete moving to the trash, and Shift+click ranges. The unmodified scenarios
must pass first; only a failed scenario assertion in the mutated run counts as
detection, not a startup/collection error or killed process. Logs and JUnit
reports are saved in `target/e2e-mutations`. The script restores the files the
patch touched afterwards, and refuses a patch whose files have uncommitted
changes. Run it after changing the harness, and when adding a scenario for a
workflow that does not have a mutation yet.

With `--qa`, each patch also runs the `--qa` scenario whose header names it
as its `mutation`, and the Python selection becomes exactly the twins that
scenario's runs print. Both must pass unmodified, and under the patch every run
must fail on both sides or on neither, with at least one failing on both; a run
where only one side fails is a parity failure:

```bash
./scripts/e2e-mutation-check.sh --qa              # every mutation
./scripts/e2e-mutation-check.sh --qa clipboard    # one of them
```

## QA scenarios (`--qa` mode)

A second runner is being added beside the Python suite, which stays the merge
gate. A build with the `qa` Cargo feature (never in releases) accepts
`strata --qa <directory>`: the normal window opens on the directory, then the
process reads one JSON command per line on stdin, runs it on the GTK main loop,
and writes one JSON reply per line on stdout after the main context drains.
Input is real XTEST sent from inside the process, and state is read from the
widget tree by the names `src/ui/accessibility.rs` sets, so no AT-SPI, D-Bus,
or Python is involved.

```bash
./scripts/qa.sh                              # every scenario, every mode
./scripts/qa.sh arrow-keys-move-focus        # one scenario, all of its runs
./scripts/qa.sh arrow-keys-move-focus/columns-ru   # one run of it
STRATA_QA_KEEP_ARTIFACTS=1 ./scripts/qa.sh   # artifacts for passing runs too
```

`scripts/qa.sh` uses the same verified image, mounts, account files, and
`target/e2e-container` cache as `scripts/e2e.sh`, and runs
`cargo test --features qa --test qa` there. Because the cache is shared, a QA
run and an E2E run each rebuild `strata` with their own features: run them one
after the other, never concurrently. `tests/qa.rs` refuses to start
without the `STRATA_QA_CONTAINER=1` that script sets. A host run needs the
owner's explicit consent for the task and `STRATA_QA_HOST=1`; it is labelled
unsupported, is never validation evidence, and must be recorded in the handoff
with the container run it replaced. The QA runner is not wired into
`scripts/e2e-native.sh` or `scripts/test-headless.py`.

Each run of a scenario gets its own Xvfb, HOME, XDG directories, and fixture
tree, materialised from `tests/fixtures/harness.json`. That manifest holds the
fixture tree, seeded preferences, GTK settings, static environment, and
keyboard layouts, and the Python harness reads the same file
(`harness/manifest.py`), so the two never depend on separate generators
agreeing; `tests/e2e/unit/test_harness_manifest.py` checks what the Python
harness materialises from it.

Two differences from the Python harness's environment change no rendering and
no behaviour a scenario checks. The QA application's `MESA_SHADER_CACHE_DIR`
names one directory the runner makes under `/tmp` and removes when it exits,
so the software GL shaders GTK compiles at start-up are built once per
`./scripts/qa.sh` rather than in every run's fresh `XDG_CACHE_HOME`, where a
twin's Mesa still keeps them. The rendering pins (`GSK_RENDERER`,
`LIBGL_ALWAYS_SOFTWARE`, `GALLIUM_DRIVER`) are the manifest's on both sides,
and helper and `foreign` processes a step starts keep the twin's environment,
without the variable. And `scripts/qa.sh` mounts a tmpfs on the container's
`/tmp`, which holds every run's fixture, HOME, and runtime directory, where
`scripts/e2e.sh` keeps the image's overlay. `/tmp` is then a mount point of
its own, the one Strata resolves for a path there instead of `/`, and a
directory may list its entries in another order; but tmpfs and overlay are
both virtual file systems, and fixtures and HOME share one device on both
sides, so Trash and its restore behave alike, and `/dev/shm` is still another
volume. Nothing in `/tmp` outlives the container, since artifacts and the
Cargo cache are under `/workspace/target`. The tmpfs takes the kernel's
default limit, half of RAM; a full run at 24 threads peaked near 150 MB.

A scenario is a `.jsonl` file that ports one Python test function, or tests
behaviour of its own. One with a `twin` lives in `tests/qa/scenarios/<test
file>/`, the folder named after its twin's file (for example
`tests/qa/scenarios/test_clipboard/copy-leaves-source.jsonl`); one without a
`twin` is QA-only and lives in a feature-area folder named in snake_case
(lowercase letters and digits joined by single underscores) without the
`test_` prefix a twin's file carries, such as
`tests/qa/scenarios/hidden_files/hidden-files-persist-across-restart.jsonl`.
The runner finds every file there itself. Scenario names are lowercase words
joined by hyphens, since `./scripts/qa.sh` reads `_` in a name as `-`, and are
unique across folders; the runner's
`every_scenario_file_is_planned_and_well_formed` test checks that, that each
file sits in a folder its header allows and loads, and that every run of every
file is planned as a test. Its first line is a header:

```json
{"scenario":"arrow-keys-move-focus","modes":["columns"],"prefs":{"type_to_search":false},"layout":"us","twin":"tests/e2e/scenarios/test_keyboard_navigation.py::test_arrow_keys_move_focus_and_selection[arrows-$mode]","mutation":"keyboard-navigation","known_bug":null,"variants":[...]}
```

`modes` runs the file once per view, `mutation` names the patch that must make
it fail, and `known_bug` an issue that makes a failure the expected result.
The optional `twin` is the exact pytest node id each run ports: `$mode` (`columns`, `icons`,
`list`) and a variant's `vars` are replaced per run, and a variant whose node
does not fit the pattern sets its own `twin` (only when the header has one);
`mutation`, `state_parity_ignore`, and `input_parity_ignore` also need a
`twin`.
Every run must resolve to a
distinct node id, and the runner prints it beside each result, as in
`PASS copy-leaves-source/columns-ru (twin tests/e2e/scenarios/test_clipboard.py::test_copy_leaves_the_source_in_place[columns-ru])`;
a run of a scenario without a `twin` is named alone, as in
`PASS hidden-files-persist-across-restart/list`, takes a fresh fixture name
even under `STRATA_PARITY_FIXTURES=1`, and writes `"twin":null` in its
`result.json`.
The optional `quality`
(`{"impact":3,"coverage":3,"criticality":3,"diagnostic":3,"score":25,"note":"..."}`)
rates the scenario: `impact`, `coverage`, `criticality`, and `diagnostic` are
integers from 1 to 5, `score` an integer from 0 to 100, and `note` one
non-empty line without any break Python's `str.splitlines` splits on
(`\n`, `\r`, U+000B, U+000C, U+001C–U+001E, U+0085, U+2028, U+2029); the
runner rejects any other key, a missing one, or a value out of range, and
checks the score exactly:
`P = 3(impact - 1) + 3(criticality - 1) + 2(diagnostic - 1)` and
`score = (50(coverage - 1)P + 32) // 64`, a mismatch failing the load with the
expected score. The `qa-quality` agent skill (`.agents/skills/qa-quality/`)
defines what the ratings mean.
A `skip` (`"skip":"<reason>"`) keeps a scenario without a `mutation` or
`known_bug` out of the default `./scripts/qa.sh` run, which prints
`SKIPPED <scenario> (run it by name): <reason>` in its place, and out of
parity, whose summary lists it with the reason; naming it,
`./scripts/qa.sh <scenario>`, still runs it (see "Skipped scenarios").
A `known_bug` needs `known_bug_step`, the file
line (the header is line 1) of the step expected to fail, never a `layout`,
`flag`, `note`, `slice`, `increment`, `stem`, `parent`, `screenshot`, `restart`,
`http`, or `foreign_stop` step; a failure at any other
step, or an error, still fails the run. The header `layout` is switched to once the
process is ready, so key names always mean US key positions. Optional `variants`
run the file again with their own `prefs`, `layout`, `modes`, `vars`, and
`twin`, and their own `tree` and extra `setup` actions, as an indirect
`fixture_tree` parametrization gives each case its own files, and extra
`teardown` actions taken after the header's, as such a case's `finally:`
restores its own directory. In a step, `$name` is replaced by a variable (`$$` is a literal `$`); the built-ins are `$root`
(the fixture directory's name, which is also its pane's), `$fixture` (its
absolute path), `$fixture_quoted` (that path as `pathlib.Path.as_uri` escapes
it, without `file://`, for a trash file's `Path=` or a URI a twin builds with
`quote_from_bytes`), `$home`, `$data`, `$config`, and `$cache` (the run's HOME and
XDG data, config, and cache directories), `$python` (the interpreter a twin's
`sys.executable` names in the pinned image, `$STRATA_E2E_VENV/bin/python`), `$environment` (the directory holding them, a twin's
`test_environment.root`), `$view` (`Columns`, `Icons`, `List`), and `$mode`.
An optional `teardown` lists `fs` actions taken once setup has begun, however
the run ends, as a twin's `finally:` restores a mode it changed. An optional
`setup` lists `fs` actions taken after the tree is written and
before the launch, as a twin's fixture writes an executable into the tree or
desktop entries and actions under `$data` and `$config` before `strata`
starts. An optional `tree` names
a manifest tree or writes one out, where a name such as `{000..199}.txt` stands
for one zero-padded file per number whose text has `{}` replaced by the number,
and a file's text may be a list of parts: strings and
`{"repeat":"record-{},{}\n","from":0,"to":999}`, the text once per number, as a
fixture joins a comprehension;
`images` (`{"beta/thumb.png":{"width":64,"height":64,"rgb":[230,40,60]}}`)
then adds solid-colour PNG images, as a fixture saves a PIL image, and `gtk_settings`
(`{"gtk-enable-animations":"true"}`) rewrites those lines of the manifest's
GTK `settings.ini`, as a twin's `test_environment` override does.
`env` (`{"PATH":"$environment/launcher-path"}`) adds or replaces variables
in the application's environment, with `$variables` replaced, as a twin's
`test_environment.variables` override does (never one the runner owns: the
display, buses, runtime, HOME and XDG directories, input log, Mesa shader
cache, and parity and state-dump variables), and `"session_bus":true` gives
the run a private session bus with the Python harness's configuration (no
service directories), which the application and `dbus_send`, `launch`, and
`foreign` steps share; without it the application gets a bus address that
cannot connect. `"portal":true` (with `session_bus`) starts
`strata --qa --portal`, the FileChooser portal backend, in place of the
browser, as `open_file_request` starts `strata --portal`: it has no window
until a `portal_open` request opens a chooser, which `adopt` then drives, and
while no chooser is open its state snapshot is the empty one AT-SPI reads.
`"fixture_root":"/tmp/strata-e2e-baseline"` builds the tree at that fixed
path, directly in `/tmp`, instead of a fresh or parity-named one, as a
golden-screenshot twin's `FixtureTree.create_at` does because the breadcrumb
and context menu render the path; a directory already there is a setup error,
never reused, and runs that claim a fixed root take turns.
`"cost":{"columns":2.4,"list":0.3}` gives each run, by its label, its measured
relative cost, which decides when it starts; `scripts/qa_cost.py` writes it
(see "Scheduling and run costs").
`state_parity_ignore` (`["focused","panes.focused"]`) leaves compared keys,
or a compared key of each pane, out of the scenario's final-state parity; it
is only for a scenario whose app state ends nondeterministically under both
harnesses (see "QA parity"), and `tests/qa.rs` and `scripts/qa_parity.py`
both reject any other key. `["*"]`, alone, leaves the whole final state out,
for a twin that ends its own application before its state can be read. `input_parity_ignore`
(`[{"run":"columns-wheel","events":[{"position":19,"event":"button 1 down"}]}]`)
likewise leaves the pointer position of each named event of one run out of
input parity: `position` counts the run's `key` and `pointer` lines from 1
(on the Python side), and `event` is what must stand there on both sides, so a
step added before it fails parity instead of moving the exemption:
`button <n> down|up` is compared without its position, `key down|up` without
its keycode, and `typed digits` starts a run of digit key presses that may be
of different lengths on the two sides, after which both logs are compared
again from the ends of their runs. The event's button or direction, and every
other event, are still compared. It needs an
owner decision, is only for a scenario without a `mutation`, and may not cover
every event; a position past either log disagrees (see "Nondeterministic input").
Every later line is one step; a step with `"modes":["icons","list"]` runs only
in those views, which is how a scenario mirrors a twin's `if mode == ...`; a step
with `"variants":["row-space"]` runs only in those header variants, as a twin's
`if target == ...`; a step with `"when":PREDICATE` runs only if the
predicate holds just before it (the runner asks the process with
`{"holds":PREDICATE}`), which mirrors a twin's `if node.has_state(...)`; and a
step with `"if":"returned"` or `"unless":"returned"` runs only if an earlier
`expect` (or `flag`) kept as `"as":"returned"` held, or did not, which mirrors a twin that
collects failures and recovers, as `if _focus_returned(...)` and
`except TreeTimeout: failures.append(...)` do. A list (`"if":["a","b"]`) needs
every one to have held, and `"unless":["a","b"]` skips the step if any did; an
`expect` or `flag` that keeps a flag and is itself skipped this way keeps it
false, so a twin's retry loop unrolls into guarded attempts.
A `note` keeps what an accessor reads, as a twin keeps
`names = strata.entry_names()`: later steps say `$names`, which becomes the
value itself where a string is exactly `$names` (so a list stays a list) and
its text elsewhere (a list as Python prints it). A note may only be used after
the step that keeps it, may not hide a variable, and any other unknown `$name`
fails the scenario before it runs.

| Step | Runs in | Meaning |
| --- | --- | --- |
| `{"key":"ctrl+x","repeat":2}` | process | US key positions, as in `harness/interaction.py` |
| `{"type":"nested-notes"}` | process | `keyboard.type_text`: one key per character |
| `{"click":TARGET,"button":1,"count":1,"mods":["shift"],"at":[4,"50%"]}` | process | `pointer.click`, `right_click` (`button` 3), `double_click` (`count` 2), `click_entry_with`; `"release_mods_first":true` is `click_releasing_modifiers_before_up`; `"left_bound":"browser"` (also on `move`) keeps the aim's left edge right of a remembered box, as `max(bounds.x, browser_left) + 12` |
| `{"select":{"entry":"readme.md"}}` | process | clicks until the entry is selected, as `select_entry` does |
| `{"select_with_keyboard":"todo.txt","message":"..."}` | process | `select_entry_with_keyboard`: Home, then Down (Right in Icons) until the entry has focus |
| `{"open_menu":TARGET,"until":PREDICATE,"message":"..."}` | process | `open_appearance_menu`: clicks, and clicks again each second the menu stays closed while the target is not checked; `"open_states":["checked","pressed"]` counts either state as open, as `test_accessibility._open_choice` does |
| `{"adopt":"PNG conversion test","timeout_ms":20000,"message":"..."}` | process | a portal run's chooser: waits for a showing application window with this title, as `application.find(name=title)`, and reads state, resolves targets, and logs input relative to it from then on |
| `{"reveal_control":{"role":"spin button","name":"Text size in pixels"}}` | process | `test_text_size._reveal_page_control`: finds the control presented (`"{name} control"`), then while its scroll pane does not show all of it turns the wheel 16 pixels inside the pane's right edge at its middle height, `min(12, max(1, int(distance / height) * 3))` clicks toward it (`"reachable {name} control"`), and settles it |
| `{"reveal":{"role":"button","name":"Modified date format"}}` | process | `Strata.reveal`: finds the control presented; unless the window and its scroll pane show it, turns the wheel three clicks at the window's centre up to twelve times, toward it, settling it once shown, then waits for it to be on screen (GTK's bridge declines the `scroll_into_view` before the loop) |
| `{"keep":[0,0,240,80],"as":"foreign"}` | process | keeps a box on the X screen for `kept` targets; the runner sends it for a `foreign` drag source's button |
| `{"activate":{"role":"toggle button","name":"..."},"message":"..."}` | process | `Node.activate`: the default action, no pointer input; a role target without `within` may be scrolled away; one with `within`, or a `{"dialog_button":"Close dialog"}`, is looked up on screen first |
| `{"holds":PREDICATE}` | process | `true` or `false` in `result`, never a failure |
| `{"remember":{"field":true},"as":"filter"}` | process | keeps the focused field, as a twin keeps `field = strata.editable_field()`; any other target keeps its box, for `left_bound` and `moved`, and its widget, for `replaced` and a `remembered` target, apart from the fields, so a field lookup never resolves it |
| `{"note":"entries","dir":"$root","as":"names"}` | process | an accessor's value for later `$names`: `directory`, `pane`, `panes`, `entries`, `selected`, `all_selected`, `matches`, `focused`, `focused_pane`, `visible` (the rows wholly inside the pane's scroll viewport, `_visible_entries`), `menu_items` (the open menu's items, empty with none), `editable_fields` (how many editable text fields are on screen, a number); `from`/`through` slice a list as `names[names.index(a):names.index(b) + 1]`, then `"middle":3` keeps `names[len // 2:len // 2 + 3]`, `"plus":["todo.txt"]` makes it `sorted(set(names) \| {...})`, `"sorted":true` sorts it, and `"pick":-1` keeps one name, `names[-1]`; `bounds` with `"of":TARGET` is its box as Python prints `screen_bounds()`, for a message; `"until_found":true` (with `message`, `timeout_ms`) polls until it is not null, as `strata.wait(accessor)` returns it; `middle_visible` with `unless_first` is the reversing-scroll scenario's `middle_entry`; `scroll_target` with `"of":TARGET` is the pane's scroll value plus `max(of.y + of.height - viewport.y + 1, 1)` |
| `{"note":"number","expr":NUMBER,"as":"fitted"}`, `{"note":"visible_top","as":"rows"}` | process | a number worked out now (see `compare`), as a twin keeps `fitted = width`; with `until_found` and `"until":[{"op":">","than":300}]` it polls until the number itself is within the bounds and keeps the value that poll read, as `strata.wait(lambda: (width := saved()) > 300 and width)`; the deepest pane's rows whose top edge lies inside its scroll viewport, as `test_monitor_selection.visible_entries` (a list, so `pick` and `middle` apply) |
| `{"increment":"first","as":"next"}`, `{"stem":"source","as":"stem"}` | runner | a noted name with its trailing number one higher, `f"folder-{n + 1:03}"`; a noted name's `Path(name).stem` |
| `{"clipboard_set":"selection-cleared"}`, `{"clipboard":"999","message":"..."}` | process | `table_clipboard_text`: opens a second connection to the display and owns the clipboard with the text, as `before_read` sets it (kept for the large-table twin listed under "Python-only tests", should it return); reads the clipboard through that connection (or a new one), closes it, and fails unless the text matches, exactly or as `{"starts_with":...,"contains":...,"longer_than":5}` |
| `{"slice":"rows","middle":3,"pick":-1,"as":"last"}` | runner | part of a noted list, so one snapshot serves several values as a twin reuses `rows`: `middle` then `pick`, as a note shapes a list |
| `{"keys_until":PREDICATE,"keys":["Home","Left"],"times":2}` | process | presses the keys in turn for at most `times` rounds, stopping once the predicate holds right after a round, as `for _ in range(2): ...; if node.has_state(...): break`; `"check_first":true` checks before each round instead, as `for _ in range(30): if ...: break; press(...)`, and `times` may be a noted list, whose length it then is, as `for _ in strata.menu_items():`; `"await_focus_change":{"timeout_ms":5000,"message":"..."}` waits after each round for the focused widget to change, as `_tab_to` waits for `focused_node()` to differ, and `"exhausted":"..."` fails with that message when every round ran without the predicate holding, as a loop's `else: raise AssertionError(...)` |
| `{"move":TARGET,"at":[...]}` | process | `pointer.move_to` |
| `{"move_by":[2,0]}` | process | `pointer.move_to` the pointer's own position plus an offset |
| `{"wheel":TARGET,"clicks":3,"direction":"down","at":[...]}` | process | `pointer.scroll` |
| `{"wheel":TARGET,"clicks":10,"until":PREDICATE,"tries":40}` | process | turns the wheel only while the predicate does not hold, checking before each turn: at most `tries` turns with no pause, as a twin's `for _ in range(40)` loop, or else (with `message`, `timeout_ms`) polling as a `strata.wait` whose condition scrolls, failing on timeout |
| `{"key_down":"ctrl"}`, `{"key_up":"Escape"}` | process | `connection.key(keysym, True/False)`: one modifier or key, no tap and no pause |
| `{"during":PREDICATE,"ms":1200,"every_ms":50,"message":"..."}` | process | checks every `every_ms` (default 50) for `ms` and fails as soon as the predicate does not hold, as a twin asserts through a sustained hover |
| `{"capture":TARGET,"inset":3,"as":"before"}` | process | keeps the screen's pixels in the target's box, inset on every side, as a twin crops a screenshot for `repainted`; any part off the screen is black, as PIL's `crop` pads it, and so it is for `repainted` |
| `{"measure_text":"label","as":"resting"}` | process | `text_position` over a kept box from a fresh screen grab: the luma-weighted mean row of the ink and its contrast; fails as `the source label disappeared` when there is no ink, and is an error when the box leaves the screen, as indexing the twin's screenshot past its edge raises (so for `drop_samples`) |
| `{"drop_samples":"label","resting":"resting","dragging":"dragging","escape":true,"as":"released"}` | process | the drag-completion release: Escape down and up when asked, then button 1 up, with no pauses, then grabs of the kept box 60, 120, and 180 ms later; each must come within 240 ms (failing with the samples printed as the twin's `assert ..., samples` prints them), sit within 1.5 pixels of `resting`, and the last must have 1.25 times `dragging`'s contrast; the release instant is kept for `elapsed` |
| `{"drag":{"from":TARGET,"to":TARGET},"steps":16,"release":true,"mods":[]}` | process | `pointer.drag`; `from` starts on its first rendered image, else its centre (`drag_origin`), except a point-like `from` (`kept`, `point`, `pane`, `background`, `window`), which aims where it always does; `release:false` keeps the button held |
| `{"drag":{"from":TARGET,"from_at":[...],"to":TARGET,"to_at":[...]},"after_press":PREDICATE,"message":"..."}` | process | `drag_points(start, end, after_press=...)`: the predicate is checked once after the press; a mismatch lets go and fails the step; `after_press` may instead list `{"wait":PREDICATE,"message":...}` and `{"expect":PREDICATE,"message":...}` steps, run in order, as a callback that waits and then asserts. Without `from_at`, the target is read before the source, as `Pointer.drag` reads both boxes once both lookups returned |
| `{"button_down":1}`, `{"button_up":1}` | process | `connection.button(n, True/False)` where the pointer is; `button_up` also ends `abandon_drag` |
| `{"set_scrollbar":"horizontal","value":0}` | process | `Atspi.Value.set_current_value` on the first scroll bar with that orientation; no pointer input |
| `{"sleep":600}` | process | `time.sleep(0.6)`; the main loop keeps running |
| `{"resize":{"width":420,"height":300}}` | process | `resize_surface` on the main window; without `height`, its current height |
| `{"settle":TARGET,"timeout_ms":15000,"as":"entered"}` | process | `strata.settle`: waits for the bounds to repeat three polls and remembers them, under `as` too when given |
| `{"wait":PREDICATE,"timeout_ms":15000,"message":"...","samples":10}` | process | stops the run on timeout, with the widget tree; `samples` polls in a row must hold, as a twin's steady-sample condition (both poll about every 50 ms, `tree.POLL_INTERVAL`); `"settling":TARGET` settles the target before every poll, as a condition that calls `strata.settle(icon)` before reading the screen |
| `{"click_result_rows":{"selected":true},"mods":["ctrl"]}` | process | each of `result_rows` (the `Search results` rows, or else every list item) whose states match when its turn comes, clicked in DFS order, as `for row in rows: if row.has_state("selected"): pointer.click(row, modifiers=("ctrl",))` |
| `{"expect":[PREDICATE,...],"message":"..."}` | process | reports mismatches and continues; `message` names the twin's assertion message; with `timeout_ms` it polls until all hold first, as a twin's `try: strata.wait(..., timeout=4) except TreeTimeout: failures.append(...)`, and with `hold_ms` too they must hold that long in a row, else hold at the last look, as `_settles(condition, timeout, hold)`; `"as":"name"` keeps whether it held for a later step's `if` or `unless` |
| `{"query":"state"}`, `"tree"`, `"prefs"` | process | snapshot, widget tree, or `settings.toml` as JSON in `result` |
| `{"wait":PREDICATE,"never":PREDICATE,"never_message":"..."}` | process | as a twin records what each poll saw and asserts on the record once its wait returns: the wait still fails if `never` held at any poll, the last included |
| `{"during":[{"check":PREDICATE,"message":"..."},...],"ms":7000,"every_ms":40}` | process | `during` over several assertions in order, each failing with its own message, as a twin's loop body asserts one after another |
| `{"note":"entries","until_found":true,"until_names":NAMES,"as":"names"}` | process | polls until the list matches and keeps the list that poll read, as `strata.wait(lambda: names if len(names := strata.entry_names()) > 16 else None)` |
| `{"each":"names","as":"name","derive":{"caption":{"number_plus":320,"suffix":"×180"}},"step":STEP}` | runner | the process step once per item of a noted list, `$name` the item and each `derive` variable the last number in the item's stem plus `number_plus` between an optional `prefix` and `suffix`, as `for name in names: strata.wait(..., f"... {name}")` |
| `{"restart":true}` | runner | `application.stop()` then `start()`: SIGTERM to the application's process group, 5 s, then SIGKILL, as `harness.process.terminate` does; it starts again with the same environment, fixture, display, and input log, under the US layout, and a fresh log, as `ManagedProcess.spawn` opens it |
| `{"restart":{"directory":"$fixture/target","env":{"RUST_LOG":"trace"},"stdout":"closed","stderr":"null"}}` | runner | as a twin stops the application and starts `strata <directory>` itself: in that directory, with those variables added, and with standard output and error a pipe whose reading end is closed or `/dev/null`, as `Popen(stdout=PIPE)` closed at once or `DEVNULL`; the process sets them up itself (`STRATA_QA_STREAMS`) before anything writes, keeping its replies on their own descriptor |
| `{"launch":["$fixture/requested-missing"],"percent_path":true}` | runner | `subprocess.run([strata, *arguments], cwd=fixture, check=True, timeout=30)` with a twin helper's environment: another `strata`, which hands its arguments to the running one over the session bus and exits; `percent_path` makes `%XX` raw bytes |
| `{"portal_open":"Multiple selection test","options":{"current_folder":"$fixture","multiple":true,"filters":[["PNG images",[[1,"image/png"]]]]}}` | runner | `open_file_request`: waits up to 20 s for the backend's bus name, then sends `OpenFile` (60 s D-Bus timeout) from a worker thread without waiting for the answer; a request still unanswered when the run ends gets `Request.Close` once the final state is read |
| `{"portal_response":{"done":false},"message":"..."}`, `{"portal_response":{"done":true,"status":0,"names":[...],"paths":[...],"keep":"output","png":{...},"discard":true},"timeout_ms":20000,"message":"..."}` | runner | `assert not response.done()`; or waits for the answer and checks its status, the chosen files' names as a set or paths in order, keeps the first path as a note, checks it as `png` checks a file, and then (`discard`) removes its download directory however that went, as `finally: shutil.rmtree(output.parent)` |
| `{"http":"<base64>","as":"url"}`, `{"http_requests":1,"message":"..."}` | runner | `image_server(body)`: an HTTP/1.0 server on 127.0.0.1 answering every GET with the body as `image/png`, its URL kept as a note, until the run's final state is read; `assert len(requests) == 1, message` |
| `{"foreign":{"paths":["$fixture/a"],"move_only":true},"as":"foreign","message":"..."}`, `{"foreign_label":"Foreign drag finished","message":"..."}`, `{"foreign_stop":true}` | runner | `tests/e2e/fixtures/file_drag_source.py` run unchanged through `tests/qa/foreign_drag_source.py`, which prints its button's screen box and each new label on stdout: waits for the box, as the twin waits for `"Foreign file selection"` on the desktop, and keeps it in the process under `as` for a `kept` drag source; waits for the label; `terminate(source.popen)` |
| `{"zipcrypto":{"path":"password.zip","password":"zipsecret","member":"some.txt","contents":"...","deflated":true},"as":"collision"}` | runner | `_write_zipcrypto` (a random 11-byte header, raw deflate at level 9) and `_zipcrypto_crc_collision`: the first numeric password below 4096 whose decrypted header passes `zipfile`'s check, kept as a note; `contents` may be `{"repeat":"text","count":64}` |
| `{"fail":"the compress dialog never accepted the archive name"}` | runner | fails with the twin's message, as a retry loop's final `raise AssertionError(...)` |
| `{"parent":"output","as":"output_dir"}` | runner | a noted path's `Path(name).parent` |
| `{"dbus_send":["--print-reply","--dest=...","/path","iface.Method","array:string:file://..."]}` | runner | `dbus-send --session ...` on the run's private bus (`session_bus`), with the environment a twin's helper gets, failing as `subprocess.run(..., check=True, timeout=30)` |
| `{"log":{"count":"browser sandbox started"},"as":"initial"}`, `{"log":{"count":"...","at_most":2},"timeout_ms":0}` | runner | reads the application's log with its colour codes stripped, as `strata.application.log()`: `count` occurrences of a text, `matches` (the numbers a pattern's first group finds, `re.findall`; `pick` keeps one), or `contains` (kept as a flag); with `as` it keeps the value as a note, without it waits until the count (or how many numbers) is `equals`, `at_least`, or `at_most` a number or a noted list's length, plus `plus` |
| `{"proc":{"gone":"$original"}}`, `{"proc":{"parent_is_app":"$pid"},"timeout_ms":0}`, `{"proc":{"kill":"$pid"}}` | runner | waits until none of the noted pids runs (`/proc/<pid>` is gone); checks `PPid:` in a pid's `/proc/<pid>/status` is the application's; sends it SIGKILL at once, refusing any pid that is not the application's child |
| `{"layout":"ru"}` | runner | `setxkbmap` and a group lock on the run's Xvfb |
| `{"flag":PREDICATE,"as":"enabled"}` | runner | keeps whether the predicate holds now, never a failure, for later steps' `if` and `unless`, as a twin reads `enabled = "pressed" in node.states` and branches on it; with `timeout_ms` it polls until the predicate holds or time runs out, as `try: strata.wait(..., timeout=5) except TreeTimeout: pass` |
| `{"fs":{"path":"archive/todo.txt","exists":true},"timeout_ms":15000,"message":"..."}` | runner | waits for `exists`, `text`, `names`, `is` (`file`, `dir`, `symlink`), `empty`, or `unchanged` (equal to a remembered listing); `timeout_ms` 0 is a plain assertion |
| `{"fs":{"path":"archive/todo.txt","write":"existing\n"}}`, `{"fs":{"path":".","remember":"original"}}` | runner | acts at once: writes a file, or keeps `fixture.listing()` under a name |
| `{"fs":{"path":"archive/folder-{000..159}","mkdir":true}}`, `{"fs":{"path":".","populate":TREE}}` | runner | `Path.mkdir()` (`"parents":true` for `mkdir(parents=True)`), once per number of a range in the last component; `FixtureTree.populate`, which also pins the times below `path` |
| `{"fs":{"path":"full/{000..179}.txt","write":"{}\n"}}`, `{"fs":{"path":"archive","chmod":"555"}}` | runner | one `write_text` per number of a range, `{}` in the text standing for it; `Path.chmod(0o555)` |
| `{"fs":{"path":"archive/report.md","copy_of":"documents/report.md"}}`, `"size_of"` | runner | waits until the file has the other file's bytes, or its size |
| `{"fs":{"path":".","remember_names":"original"}}`, `{"fs":{"path":".","names_from":"original","minus":["todo.txt"],"plus":[...]}}` | runner | keeps `set(fixture.names(path))`; waits until the names are that set less `minus` plus `plus` |
| `{"fs":{"path":"drop-volume","volume":"/dev/shm"}}` | runner | the cross-volume fixture: a fresh `strata-drop-volume-*` directory in `/dev/shm` (`"prefix":"strata-no-trash-"` names it otherwise, and `"with":{"doomed.txt":"doomed"}` writes files into it before it is linked, as a twin's fixture fills its temporary directory before `symlink_to`), which must lie on another device than the fixture, linked at `path` and removed when the run ends, or by `{"fs":{"path":"no-trash","remove_volume":true}}`, which leaves the link dangling as a twin's fixture teardown does before the state dump |
| `{"fs":{"path":"new file (1)","symlink":"missing"}}`, `{"fs":{"path":"run-me","program":"true"}}` | runner | `Path.symlink_to(target)`; `shutil.copy2(shutil.which("true"), path)`: the program's bytes, mode, and times |
| `{"fs":{"path":"$config/strata/settings.toml","has":{"contains":["\"$fixture/docs2\""],"lacks":[...]}}}` | runner | waits until the text contains every part and none of the others; an unreadable file contains nothing, as `_settings_has_key` treats an `OSError` |
| `{"fs":{"path":".","names_without":"clicked"}}`, `{"fs":{"path":"no-trash/doomed.txt","can_trash":false}}` | runner | waits until no name contains the part, as `not any("clicked" in name ...)`; until GIO's `access::can-trash` is that value |
| `{"fs":{"path":"todo.txt","remove":true}}`, `{"fs":{"path":"documents","rename":"archive/documents"}}` | runner | `Path.unlink()`, or `shutil.rmtree` for a directory; `Path.rename(root / ...)`; both refuse a path whose parent, with links resolved, lies outside the fixture, the run's `$environment`, and its scratch directory |
| `{"fs":{"path":"$environment/launches","lines":{"at_least":1}}}`, `{"fs":{"path":"a.png","keep_bytes":"original"}}`, `"bytes_of":"original"` | runner | waits until the file has that many lines, exactly or at least (a missing file has none; an unreadable or non-UTF-8 one fails the step at once, as `read_text()` raises), as `len(path.read_text().splitlines())`; keeps `path.read_bytes()` under a name; waits until the bytes are those kept, failing at once on a read error |
| `{"screenshot":"hover.png"}` | runner | captures the Xvfb root, as a twin's artifact screenshot; saved with the run's artifacts when it keeps them |
| `{"baseline":"columns-view"}` | runner | the `baseline` fixture: captures the Xvfb root with ImageMagick's `import`, as `strata.screenshot` does, and compares it with `tests/e2e/baselines/gtk-<major>.<minor>/columns-view.png` for the GTK the runner links, as `screenshots.compare_to_baseline` does: a pixel differs when a channel moves by more than 24, and more than 0.5% of differing pixels, a size change, or a missing image fails with the twin's summary; the run's artifacts keep `<name>.actual.png`, and on a mismatch `<name>.expected.png` and `<name>.diff.png`. It never writes a baseline. Its failure starts with that summary, as the twin's assertion does, so the step carries no `message`; the percentage in it differs from run to run, and as the baseline scenarios have no `mutation`, their failure signatures are never compared |
| `{"fs":{"path":"photo.png","image":{"width":1600,"height":900,"rgb":[0,128,0]}}}`, `{"fs":{"path":"sound.wav","ffmpeg":["-f","lavfi",...]}}` | runner | a solid-colour PNG, written in place in one write as PIL's buffered save writes it (written in pieces, the listing would see it grow and replace its row), or `ffmpeg -v error -y <ffmpeg...> <path>` |
| `{"fs":{"path":"small.png","thumbnail":{"cache":"$cache/thumbnails/large","width":256,"height":128,"rgb":[0,0,255],"mode":"700"}}}`, `{"fs":{"path":"photos-a/model.3mf","cached_in":"$cache/thumbnails/large","pixel":{"at":[0,0],"rgb":[220,40,80]}}}`, `{"fs":{"path":"photos-a","all_cached_in":"$cache/thumbnails/large","except_suffix":".stl"}}` | runner | writes a freedesktop cached thumbnail for the file (a solid-colour PNG named after the MD5 of its URI, with `Thumb::URI` and `Thumb::MTime` text chunks, in one write, `mode` setting the cache directory's); `cached_in` reads the path's cached thumbnail instead for `exists`, `is`, `pixel`, `keep_mtime`, and `mtime_of`; waits until every entry but those ending in `except_suffix` has one, as `_folder_cached` |
| `{"fs":{"path":"a.png","pixel":{"at":[0,0],"rgb":[...]}}}`, `"keep_mtime":"stored"`, `"mtime_of":"stored"`, `"executable":true`, `"mtime":1000000000` or `{"local":[2001,9,9,1,46,40]}` | runner | `Image.open(path).convert("RGB").getpixel(at)`; keeps `st_mtime_ns` and waits until it is that again; `st_mode & 0o111 == 0o111`; `int(st_mtime)`, a local time read in the runner's zone as `datetime(...).timestamp()` |
| `{"fs":{"path":"unknown.bin","hex":"0001..."}}`, `"base64":"..."`, `"append":"text"`, `"raw_photo":true`, `"pdf_image":{"width":400,"height":200,"rgb":[...]}`, `"magick":["-limit","thread","1","$fixture/small.png"]` | runner | bytes in one write; `open("a").write`; `harness.raw_photo.write_raw_photo`; a one-page PDF of a solid image at 72 dpi, as PIL saves to `.pdf`; ImageMagick (`magick`, else `convert`) with the path last |
| `{"fs":{"path":"a.tar.gz","tar_gz":{"mode":"755","mtime":1000000000,"members":[["a.txt","text"]]}}}`, `"zip_members":[["todo.txt","todo.txt"]]`, `"tar_members":[...]`, `"is_zip":true` | runner | a gzipped tar as `tarfile.open(path, "w:gz")` with `TarInfo` modes and times; the archive's member names are exactly these in order, each holding the bytes of the named fixture file; `zipfile.is_zipfile` |
| `{"fs":{"path":"$config/mimeapps.list","default_app":{"for":"todo.txt","is":"x.desktop"}}}`, `{"fs":{"path":"$environment/opened-files","opened":["$fixture/todo.txt"]}}` | runner | the first desktop file `[Default Applications]` lists for the content type GIO gives the file; the file's lines, read with `Gio.File.new_for_commandline_arg`, are as many as the paths and include each |
| `{"fs":{"path":"action.toml","toml":{"run.args":["a"]}}}`, `"toml_like":"saved"`, `{"fs":{"path":"record","json":{"action":"keep"}}}`, `"json_set":{"paths":[...]}` | runner | values at dotted keys of the parsed TOML; the parsed TOML equals that of bytes `keep_bytes` kept; values at top-level JSON keys, or each list compared as a set |
| `{"fs":{"path":"$environment/terminal-launches.log","launches":{"count":3,"within":"$fixture"}}}`, `{"launches":{"first_dir":"$fixture/documents"}}`, `{"fs":{"path":"$data/Trash/info","replace_lines":{"suffix":".trashinfo","prefix":"DeletionDate=","with":"..."}}}` | runner | the stub terminal launcher's `cwd<TAB>--dir=...` lines as `test_terminal` asserts on them; rewrites matching lines of every file with the suffix |
| `{"fs":{"path":"documents/bad%E8name.txt","write":"x","percent_path":true}}` | runner | the path's `%XX` escapes stand for raw bytes, for names that are not UTF-8 |
| `{"fs":{"path":"payload.bin","random":67108864}}`, `"size":67108864`, `{"fs":{"path":".","glob_none":".strata-compression-*"}}` | runner | `write_bytes(os.urandom(size))` in one write; `st_size == size`; no name in the directory matches the pattern, `*` any run of characters, as `not list(root.glob(pattern))` |
| `{"fs":{"path":"$config/gtk-3.0/bookmarks","lines_equal":[...]}}`, `"last_line":"..."` | runner | `path.read_text().splitlines()` equals the list, a missing file having none, as `test_sidebar_pinning.bookmarks`; its last line is that, as `bookmarks(strata)[-1]` |
| `{"fs":{"path":"settings.toml","toml_prefix":{"sidebar_order":["desktop","home"]}}}`, `"toml_like":"original","toml_keys":[...]` | runner | the list at the dotted key starts with these, as `tomllib.loads(...)["sidebar_order"][:2] == [...]`; only those keys of the TOML kept by `keep_bytes` and now compare equal |
| `{"fs":{"path":"todo.txt.zip","zip_member":["todo.txt","todo.txt"]}}`, `"png":{"size":[23,17],"at":[5,5],"min":[241,0,0],"max":[255,9,9]}` | runner | the zip's member holds the fixture file's bytes, as `archive.read(name) == path.read_bytes()`; the path ends in `.png`, is a PNG of that size, and the RGB pixel's channels lie within bounds |
| `"tar_gz":{"mode":"644","mtime":0,"members":[["a.txt",{"repeat":"x","count":50000}]],"flip_from_end":8}` | runner | a member's text repeated, and the compressed byte that far from the end flipped, as `damaged[-8] ^= 0xFF` breaks the gzip trailer's CRC |
| `{"fs":{"path":"a.3mf","zip":[["3D/3dmodel.model","text"],["thumb.png",{"image":{...}}]]}}`, `{"fs":{"path":"big.stl","truncate":134217729}}`, `{"fs":{"path":"page.pdf","from_repository":"tests/fixtures/documents/text.pdf"}}` | runner | a stored zip of the entries in order, as `zipfile.ZipFile(path, "w").writestr` (an entry may be `{"text":...,"date_time":[2001,9,9,1,46,40],"mode":"100755"}`, a `ZipInfo` with that time and `external_attr`); an `image` with a range in its name (`photo-{0..5}.png`) writes one file per number, `width_step` wider each, each followed by its `thumbnail` when given; `path.open("wb").truncate(size)`; the repository file's bytes in one write, as `write_bytes(... .read_bytes())` |

The thumbnail, archive, byte, append, line-rewriting, generated-file, and
record `fs` kinds, like `remove` and `rename`, refuse a path outside the
fixture, the run's `$environment`, and its scratch directory. A cached
thumbnail's name and `Thumb::URI` use the path escaped as
`pathlib.Path.as_uri` escapes it.

`fs` paths are relative to the fixture directory unless absolute, so
`$data/Trash/files` is the run's isolated trash. Every timeout defaults to 15
seconds, the Python suite's `tree.DEFAULT_TIMEOUT`. A missing directory counts
as `empty`, as `not (path.exists() and any(path.iterdir()))` does. A `message`
on `select_with_keyboard` or `activate` names the twin's step in its failure.

Pointer timings, the approach-then-confirm move, the drag's intermediate
points, and the pause after a release before a drag all follow
`harness/interaction.py`. A target is never a screen point; it is resolved from
the widget's bounds in its native surface plus that surface's X origin, which
covers popovers, and is waited for up to 15 seconds as the Python lookups wait.
The pointer aims only once that box comes through a painted frame unchanged: a
row a listing has just added or replaced can report a box before its first
allocation lands, which the slower Python lookups never see. A tapped key comes
up 20 ms after it went down, as `Keyboard._tap` sleeps, with the X server
timing the release (XTEST's delay), never after waiting for a frame: the
server autorepeats a key held past 660 ms, and a busy app could hold a frame
that long after a press that started a navigation.

| Target | Python lookup |
| --- | --- |
| `{"entry":"todo.txt","dir":"archive"}` | `entry(name, directory)`; any pane without `dir` |
| `{"result":"beta/match-note.txt"}` | `test_filter_results.result`: a filter result by its path |
| `{"result_row":-1,"containing":"match-note"}`, `{"result_row":0,"name":"match-note.txt"}` | `test_filter_results.result_rows`, the `Search results` rows or else every list item: one by its index in DFS order (negative counts back), among those whose name contains the part or is the name, as `[row for row in rows if "match-note" in row.name][-1]` |
| `{"role":"button","name":"drafts","nth":-1}`, `{"role":"list item","name_ends_with":["/a","/b"]}` | `window.find_all(role=..., name=...)[-1]`; `name_ends_with` keeps names ending with any of the parts, as `node.name.endswith(f"/{name}")` |
| `{"focused_widget":true}`, `{"remembered":"focused"}` | `strata.focused_node()`; the widget `remember` kept under the name, as a twin keeps `focused = strata.wait(strata.focused_node)` |
| `{"role":"check menu item","name":"Folders first"}` | `window.find(role=..., name=...)` |
| `{"role":"button","name":"Next video","within":{"preview":true}}` | `strata.preview().find(role=..., name=...)`: the search starts at another target; without `name` any name matches, as `entry.find(role="panel")`, and `description` must match too when given; `"states":["selected"]` (also `focused`, `checked`, `pressed`, `sensitive`, `horizontal`, `vertical`) as `find(..., states={...})`, and `"name_matches":"^\\d+:\\d\\d$"` as `find_all(name_matches=...)`, a regular expression searched in the name |
| `{"role":["check box","toggle button","switch"],"name":"Confirm"}` | `next(node for node in find_all(name=...) if node.role in {...})`: a role list matches any of them |
| `{"role":"*","states":["selected"],"within":{"preview":true}}`, `{"role":"list item","has":{"role":"label","name":"top.txt"}}`, `{"role":"button","name":"Settings","rendered":false}` | `preview.find(states={"selected"})`: `*` is any role; a match that has a rendered widget the inner role target finds at or below it, as `archive_row` picks the row holding a label; `find(..., rendered=False)`, presented but perhaps scrolled away, which `state`, `description`, `activate`, and `remember` then read as it is |
| `{"row":"readme.md"}` | `test_chooser_selection._find_row`: the first rendered list item with the name, else the first table cell |
| `{"other_window":"Strata"}` | a showing application window with this title other than the one the run started with, as a reveal opens a second browser window |
| `{"toplevel":"Copy to"}`, `{"field":true,"within":{"toplevel":"Copy to"}}` | `destination_chooser(title)`: an application window by its title, outside the browser window; `chooser.find(role="text", states={"editable", "focused"})`, which `remember` keeps as a field |
| `{"sidebar":"Home"}`, `{"header":"Choose sort field"}` | `sidebar_button`, `header_button` |
| `{"menu_item":"New File"}` | `menu_item`: in the open menu if there is one |
| `{"dialog_button":"Close"}` | `dialog_button` |
| `{"field":true}`, `{"field":"filter"}` | `editable_field`: the focused editable text, or the one remembered as `filter` |
| `{"background":true,"dir":...}` | `background_point`: a point, so it takes no `at` |
| `{"pane":"archive"}` | the pane; aims at `empty_point` unless `at` is given |
| `{"viewport":null}` | the scroll viewport around a pane's entry list |
| `{"entry":"todo.txt","part":"label"}`, `"part":"icon"` | `entry.find(role="label", name=name)`, `entry.find(role="image")` |
| `{"window":true}`, `{"window":true,"y_of":TARGET}` | `strata.window`; with `y_of`, the point's height is that target's centre, as `(window.x + window.width - 40, pane.center[1])` |
| `{"preview":true}` | `strata.preview()`, the quick preview drawer |
| `{"search_result":"todo.txt"}` | `search_result(name)`: a `Search results` row by name, or else any list item with it |
| `{"named":"Apply to All","within":{"dialog":true}}`, `{"dialog":true}` | `dialog.find(name=...)`: any role, inside another target; `dialog()` |
| `{"container":"archive"}` | `entry_container(directory)`: the list or grid inside a pane |
| `{"parent":TARGET}`, `{"ancestor":"scroll pane","of":TARGET}` | `node.parent`; the nearest ancestor with the role, as `node.ancestors()` is walked |
| `{"kept":"edge"}` | a box `settle` or `remember` kept, or a point `remember` kept, as a twin keeps `bounds` before the window changes |
| `{"point":[X,Y]}` | a point put together from targets, as `(pane.x + 2, container.y + 2)`; each coordinate is `{"of":TARGET,"at":OFFSET}` (its centre without `at`), `{"mid":[A,B]}` (`(a + b) // 2`), `{"max":[A,B]}`, or `{"extent":TARGET}` (its width or height taken as a screen coordinate, as `park_pointer` moves to `(width - 4, height - 4)`), each with an optional `"plus":40`, in whole pixels |
| `{"entry":"todo.txt","part":"metadata"}` | the row's first named label other than its name, as `metadata_drag_origin` finds it |

`at` is `[x, y]` from the target's top-left corner, each in pixels (negative
counts back from the far edge), a share such as `"80%"` or `"2/3"` (rounded
down, as `width * 2 // 3`), a share plus pixels such as `"2/3+3"`,
`"start-6"` or `"end+60"` for pixels from the near or far edge that may lie
outside the target, or `"end-1/6"` for a share back from the far edge; without
it the pointer aims at the centre. A share has no lower bound: the row-padding
twin's `max(1, height // 6)` equals `"1/6"` for every row at least 6 pixels
tall, and the scenarios only aim at rows taller than that. Role names are AT-SPI's, with `push button` spelled
`button`, a plain box `panel`, a group `grouping`, a scale `slider`, and a password entry `password text`, as GTK's AT-SPI bridge
reports them; names are the labels `src/ui/accessibility.rs` records (an entry row's
is its list item's, and `set_labelled_by` joins its labels' names), a label's
own text, or children's names for buttons and menu items; a menu button's inner
toggle takes the menu button's name, as GTK names it, and a button with a text
label is named by that label, through the `labelled-by` relation GTK gives it,
before any label set on the button, unless `accessibility::clear_labelled_by`
dropped the relation or `set_labelled_by` replaced it. A list view's row is
selected as its selected state flag says, as GTK reports it. Tooltips never name a
target: a control that needs a name, such as an icon-only button, gets
`accessibility::set_label` with its tooltip text. The `tree` query prints both
for every widget, which is the quickest way to write a target. `absent` holds
only when the target is not found or not on screen; a lookup that breaks, such
as an unrealized surface, is an error.

Predicates mirror the page object's accessors:

| Predicate | Python accessor |
| --- | --- |
| `{"directory":"archive"}`, `{"panes":[...]}`, `{"pane":"archive"}` | `current_directory()`, `pane_names()`, `pane().name` |
| `{"entries":NAMES,"dir":...}`, `{"selected":NAMES,"dir":...}` | `entry_names(dir)`, `selected_names(dir)` |
| `{"entry":"a.txt","dir":...}`, `{"entry_gone":"a.txt"}` | `entry(...)`, `wait_for_entry_gone(...)` |
| `{"focused":"a.txt"}`, `{"focused_pane":true}` | `focused_name()`, `focused_pane()`; a name, `true` for any, or null |
| `{"dialog":"Unable to rename item"}` | `dialog()`: its name, `true` for any, or null for none |
| `{"menu_items":NAMES}`, `{"menu_items":{}}`, `{"menu_items":null}` | `menu_items()`; `{}` is any open menu, null none |
| `{"preview":"beta source"}`, `{"preview":null}` | `preview_shows(text)`, `preview() is None` |
| `{"matches":NAMES,"dir":...}` | `matches(dir)`: the search results, or else the pane's entries |
| `{"field":{"of":"rename","name":"Rename","text":"a","caret":4,"selection":[0,4],"focused":true}}` | a field node's `text`, caret offset, selection (null for none), and `focused` state |
| `{"editor_within_viewport":{"of":"rename"}}` | `assert_editor_constrained` in the caret scenario |
| `{"pref":{"key":"browser_mode","value":"icons"}}` | `read_preferences()[key]`, parsed from TOML; null when absent; `"contains":"size"` instead holds for a list holding the string or a string containing it, as `'"size"' in read_preferences().get(key, "")`, and `"one_of":[1,2,3,4]` holds for any of the values |
| `{"state":{"target":TARGET,"selected":true,"focused":false,"checked":false,"pressed":true,"sensitive":true}}` | `node.has_state(...)` for each state given; an active switch is `pressed` as well as `checked`, and `selected` is what `accessibility::set_selected` recorded, as for a tab, as GTK's bridge reports them (a list box row and a notebook tab report their own selection); `selectable` (a list box row that takes the selection), `editable` (an editable entry or text view), `focusable` (a widget that can take focus), and `busy` (what `accessibility::set_busy` recorded, as `has_state("busy")`) too; a description is the recorded one, a picture's alternative text, or for a menu button's inner toggle the menu button's, as GTK describes it |
| `{"view_mode":"Icons"}`, `{"window_width":420}` | `view_mode()`, `window_bounds().width` |
| `{"present":TARGET}`, `{"absent":TARGET}` | `window.find(...)` is or is not `None` |
| `{"moved":{"target":TARGET,"direction":"up"}}` | bounds against the last `settle`; also `down`, `left`, `right`, or `none`; `none` with `"within":1` allows that much drift on both axes and keeps the size, as `assert_layout` |
| `{"any":[PREDICATE,...]}` | `a or b` |
| `{"not":PREDICATE}` | `not ...` |
| `{"all_selected":NAMES}` | `all_selected_names()`: every pane's selection, left to right |
| `{"focused":{"one_of":[...]}}`, `{"focused_in_matches":true}` | `focused_name() in names`, `focused_name() in matches()` |
| `{"preview":true}`, `{"peek":true}` | `preview() is not None`, `peek() is not None` |
| `{"on_screen":TARGET}` | `on_screen(node)`: within the window's height |
| `{"blank":{"target":TARGET,"at":[...]}}` | `_blank_preview_point`: the point lands on nothing inside the target but panels and fillers |
| `{"visible":NAMES,"dir":...}` | `_visible_entries`: the rows wholly inside the pane's scroll viewport |
| `{"scroll_value":{"at_least":N},"dir":...}` | the first scroll bar showing in the pane's viewport, `scroll_bar.numeric_value >= N` |
| `{"band_selected":{"from":"start","to":"end"}}` | `visible_band_is_selected`: the visible rows whose name box ends above the kept point `to` and overlaps the band from kept `from` are all selected, and there is one |
| `{"focus_within":TARGET}` | `any(node.has_state("focused") for node in target.walk())` |
| `{"resized":{"target":TARGET,"since":"sidebar","width":0}}` | the width against a kept box: exactly that much wider, or `{"min":30}` |
| `{"contains_point":{"target":TARGET,"point":TARGET,"axis":"x"}}` | `bounds.x <= x < bounds.x + bounds.width` for the point's coordinate |
| `{"footer_fill":{"ends":["001.txt","011.txt"],"total":180}}` | `footer_counts_the_remaining_fill`: both ends listed, cleared, and on screen, and a label described as `"<total - cleared> of <total> items selected"` |
| `{"repainted":{"since":"before"}}` | `visibly_hovered`: a fresh grab of a capture's box differs from it outside the pointer's square, 2 pixels before the box's centre to 32 after |
| `{"contrast_below":{"of":"dragging","to":"resting","share":0.8}}` | `dragging_contrast < resting_contrast * 0.8` for two `measure_text` results |
| `{"elapsed":{"since":"released","ms":600}}` | `time.monotonic() - released >= 0.6` |
| `{"diagnostics_mention":["could not","permission denied"]}` | any of them in `strata.diagnostics().lower()`: the accessible names and descriptions of every application window |
| `{"all":[PREDICATE,...]}` | `a and b` |
| `{"editable_fields":1}` | `len(window.find_all(role="text", states={"editable"}))` |
| `{"description":{"target":{"menu_item":"Copy"},"text":"Ctrl+C"}}` | `node.description == "Ctrl+C"`, or a pattern such as `{"contains":...}` |
| `{"mentions":{"target":{"dialog":true},"text":"Customize Folder"}}`, `"words":"doesn't support Trash"` | `"Customize Folder" in dialog.dump()`: some name or description in the subtree contains it; with `"role":"label"`, the name of some rendered widget with that role does, as `any("x" in node.name for node in dialog.find_all(role="label"))`; with `words`, some rendered widget's `f"{name} {text}"` has them with any whitespace between, as `re.search(r"doesn't\s+support\s+Trash", ...)`; with `equals`, some rendered widget's name with its whitespace collapsed is exactly that, as `" ".join(node.name.split()) == text` |
| `{"presented":{"role":"label","name":"Ctrl+Alt+M"}}` | `window.find(..., rendered=False) is not None` |
| `{"file":{"path":"$fixture/documents/notes.txt","exists":false}}`, `{"file":{"path":"$fixture/todo.txt","mode_bits":{"mask":"100","set":true}}}` | whether the path exists without following a final symlink, as the runner's `fs` `exists` checks it; or `bool(path.stat().st_mode & 0o100)`; read by the process so `any` and `all` can join it with window state |
| `{"focused_widget":"Custom color…"}` | `strata.focused_node().name`, exactly or as `{"contains":"Shortcuts"}`; null when nothing has focus |
| `{"inside":{"target":TARGET,"of":{"container":"rename-target"},"axis":"y"}}` | the target has width and height and lies inside the other's box, on both axes or just `axis` |
| `{"editor_within_window":{"of":"rename","pane":"$root"}}` | the narrow-window creation check: a remembered field wider than one pixel, starting at or right of `max(pane.x, 0)` and ending inside the window |
| `{"replaced":{"target":{"entry":"rename-target"},"since":"listed"}}` | the target now resolves to another widget than the one `remember` kept under `since`, as a reload rebuilds a listing's rows; for the QA-only waits in "QA-only waits" |
| `{"pixel":{"target":{"role":"image","within":{"result":"beta/thumb.png"}},"rgb":[230,40,60]}}` | `screenshot.getpixel(bounds.center)` equals the colour |
| `{"accent_pixels":{"target":{"field":"filter"},"inset":2,"more_than":100}}` | more than that many pixels in the target's box, inset on every side, are exactly the accent of the theme `settings.toml` names in `data/themes/catalog.toml` |
| `{"any_result_row":{"focused":true}}` | `any(row.has_state("focused") for row in result_rows(strata))`; `selected` too |
| `{"hit_path":{"mark":"search: txt","hits":{"todo.txt":"todo.txt"},"name":"todo.txt"}}` | `footer_hit`: of the panels beside the footer mark, the first whose rendered labels, joined, are one of the hits' paths shows `hits[name]`; a `name` the hits do not list is an error, as the twin's `KeyError` |
| `{"compare":{"left":NUMBER,"op":"<=","right":NUMBER}}` | `heading.x + heading.width <= button.x`: `op` is `<`, `<=`, `==`, `!=`, `>=`, or `>`; a `NUMBER` is a literal, `{"of":TARGET,"get":"width"}` (`x`, `y`, `width`, `height`, `right`, `bottom`, `center_x`, `center_y` in whole pixels, or `value`, a slider's `numeric_value`), `{"pref":"text_size","default":0}`, `{"count":{"role":"image","description":"LaTeX equation","within":TARGET,"rendered":true,"states":["selected"]}}` (`len(find_all(...))`), or `sum`, `minus`, `times`, `divide`, `floor_divide` (Python's `/` and `//`), `ceil`, and `abs` over them; a target not on screen is a mismatch, so a wait keeps polling |
| `{"panes_with":{"target":{"role":"button","name":"Refresh (F5)"},"names":["documents"]}}` | `[name for name in pane_names() if pane(name).find(...)]`: the panes, left to right, in which the role target is on screen |
| `{"largest_image":{"within":{"preview":true},"wider_than":40,"size":[160,80]}}`, `"exceeds":[160,80]` | of the rendered images inside the target wider than `wider_than`, the largest by area (the first of equals) exists, is exactly `size`, or is larger than `exceeds` on either axis |
| `{"children":{"comm_prefix":"xdg-terminal-ex","count":0}}` | `test_terminal._launcher_children`: how many of the application's child processes have a command name with the prefix |
| `{"list_sections":{"within":{"dialog":true},"section":"Recommended Applications","names":NAMES}}` | the selectable list items under each unselectable heading row, named by the row's first named child, as `test_open_with` groups them |
| `{"text":{"target":TARGET,"text":"batch-rename"}}` | `node.text`, the text interface: a label's or entry's text, a text view's buffer |
| `{"some_window":{"title":"Strata","holds":PREDICATE}}`, `{"windows":{"title":"Strata","count":6}}` | `any(... for window in application.find_all(role="frame", name="Strata"))`: the predicate read in each showing application window with the title as its own tree; how many there are |
| `{"child_names":{"of":{"parent":{"sidebar":"Home"}},"role":"button","names":NAMES}}` | `[row.name for row in node.children if row.role == "button"]`, the target's visible children, as `_place_names` lists the sidebar's places |
| `{"focus_outside":TARGET}`, `{"focused_widget_is":TARGET}` | `test_accessibility._focus_outside`: the focused widget has a name and is neither the target nor inside it; `strata.focused_node() == target` |
| `{"alive":"rename"}`, `{"subtree_text":{"target":{"dialog":true},"contains":"...","strip_newlines":true}}` | `node.alive` for a widget `remember` kept: it still belongs to a window; `" ".join(node.text for _, node in dialog.walk() if node.text)` contains the part, after `.replace("\n", "")` with `strip_newlines` |
| `{"name":{"target":TARGET,"text":"0:05"}}`, `{"editable_text":"todo"}`, `{"visible_top":NAMES}` | `node.name == "0:05"` (or a pattern; with `"rendered":false` the target is looked up presented, as `find(..., rendered=False).name`); `any(node.text == "todo" for node in window.find_all(role="text", states={"editable"}))`; the rows `note visible_top` lists |

`NAMES` is an exact list in visual order or
`{"contains":[...],"excludes":[...],"count":4,"at_least":2,"ends_with":[...]}`,
with `"first"` (`names[0] == ...`), `"first_above"` (`names[0] > "010.txt"`),
`"any_from"` (some name `>= "040.txt"`), and
`"starts_with":[...]` (`names[:2] == [...]`),
`"toggled":{"names":[...],"against":[...]}` (each listed exactly when
`against` does not list it), `"in_order":[...]` (each listed, first
occurrences in that order, as `assert_menu_order`), `"set":[...]`
(`set(names) == {...}`), `"occurrences":{"a.txt":2}`
(`names.count("a.txt") == 2`), and `"sorted_casefold_from":1`
(`names[1:] == sorted(names[1:], key=str.lower)`), comparing names as Python compares strings;
`panes` takes one too. `moved` takes `"since":"entered"` to compare with a box
`settle` or `remember` kept under that name, and with direction `none`,
`"y_within":2` allows that much vertical drift and ignores the rest, as
`abs(restored.y - y) <= 2`. A `field` check also takes `showing` (the field is
mapped), and `text` also `{"lowercase_contains":...}`. A `field` check reads the
field remembered as `of`, else the first on screen with that accessible `name`
(never both), else the focused one; each property it gives must match, `text` exactly or as
`{"starts_with"|"ends_with"|"contains":...}` and `caret` exactly or as
`{"min":1,"max":72}`. Caret and selection come from `gtk::Editable`, which a
`GtkEntry` such as the inline rename editor delegates to its `GtkText`, so they
are the offsets AT-SPI's text interface reports.

### Scheduling and run costs

`tests/qa.rs` makes every run of every scenario a test of its own, named
`<scenario>/<run>` as in its `RUN` line (`copy-leaves-source/columns-ru`), so
the runs of one scenario spread over the threads, each with its own Xvfb and
process. Its harness, `tests/qa/harness.rs` (`harness = false`), takes
libtest's command line and output but starts the tests in cost order, not
alphabetically: costliest first, a run without a `cost` before all of them
(it may be costly), and equal costs by name. It runs `--test-threads` tests at
once, one per CPU by default. A run with a `fixture_root` never starts while
another such run is going; the next run free to start takes the thread
instead of waiting. A run's cost outside its steps (Xvfb, the launch up to
the first reply, and shutdown) is about 0.4 s at 24 threads, so it takes no
display lock and no 50 ms exit poll: concurrent Xvfb starts begin at
different display numbers, skip one with a lock file or socket, and try the
next when another server takes theirs first, as `-displayfd` shows; an
exiting application, Xvfb, or bus is awaited on its pidfd; and `strata --qa`
checks that its window is ready after each frame it paints. With `--exact`,
which `./scripts/qa.sh` adds when it names anything, a scenario name selects
all of its runs and never another scenario it prefixes, and
`<scenario>/<run>` selects one run; without it a filter is a
substring, as in libtest. `./scripts/qa.sh --skip <scenario>` (or
`<scenario>/<run>`, in either spelling) passes the harness's own
`--skip-exact`, which leaves out that scenario or run and never another
scenario it prefixes; any other `--skip` is libtest's substring, or a whole
name once a scenario is named. Nothing is captured: as each test finishes, the
harness prints its result line with the time it took,
`test <name> ... ok <12.345s>` (or `FAILED <12.345s>`), and the run's `PASS`
or `KNOWN BUG` line, each line whole, and at the end the failure details and
libtest's `test result:` summary. `--list` prints the selected tests in cost
order, which is the order they are offered in; a run with a `fixture_root`
may start later while another such run is going. `--help` lists the options;
libtest's `--report-time`, `--shuffle`, and `--shuffle-seed` are accepted and
change nothing, as cost order wins.

The runner's own unit tests live in `tests/qa/scenario_tests.rs` and
`tests/qa/step_tests.rs`, and each must be listed in the `unit_tests!` macro in
`tests/qa/unit_tests.rs`: with the harness replaced, `#[test]` and
`#[cfg(test)]` there are stripped without a warning and never run, so the
`no_test_attribute_hides_a_runner_test` test rejects them in `tests/qa.rs` and
`tests/qa/*.rs`.

A header's optional `cost` maps every run label of the scenario, and nothing
else, to a positive number: the run's median wall-clock time over several full
runs divided by the median of all runs' medians, so 1.0 is a typical run,
rounded to one decimal place and at least 0.1. The runner rejects a `cost`
that misses a run or names another. Refresh the costs after adding scenarios
or runs, or when the order no longer fits:

```bash
python3 scripts/qa_cost.py run                               # three full runs, then every header's cost
python3 scripts/qa_cost.py run --repeat 5
python3 scripts/qa_cost.py write target/qa-cost/run-*.log    # again from saved qa.sh logs
```

`run` times full `./scripts/qa.sh` runs at the default thread count, naming
every scenario so a skipped one is timed too, and keeps their logs in
`target/qa-cost/`. A failed run's time counts like a passing one's, and
nothing is written unless every planned run has a time. Only the `cost` value
of each header line changes, added last where there is none; every other byte
of the files stays. The order matters more than the numbers: a new run without
a cost starts early until the next refresh.

### Replies and the state snapshot

A reply is `{"ok","app","changed","criticals","result","failures","error"}`.
`changed` lists the `app` keys that differ from the previous reply, and any GLib
warning or critical in `criticals` fails the run. `app` has exactly these keys:

```json
{
  "view_mode": "Columns",
  "directory": "strata-qa-fixture-ab12CD",
  "panes": [
    {"name": "strata-qa-fixture-ab12CD", "selected": ["archive"], "focused": null},
    {"name": "archive", "selected": [], "focused": "todo.txt"}
  ],
  "focused": "todo.txt",
  "focused_pane": "archive",
  "dialog": null,
  "menu": null,
  "preview": null,
  "field": null,
  "matches": ["todo.txt"]
}
```

- `view_mode`: `view_mode()`, the first pane's presentation (`Columns`, `Icons`,
  `List`), or null with no pane on screen.
- `directory`: `current_directory()`; `focused_pane`: `focused_pane().name`.
- `panes`: `containers()` left to right; per pane its name, `selected_names` in
  visual order, and the name of its entry holding focus or null. As through
  AT-SPI, a focused widget that is not mapped, such as a row a list keeps
  while it is scrolled away, holds no focus.
- `focused`: `focused_name()`.
- `dialog`: `dialog().name`, or null.
- `menu`: `menu_items()` of the open menu, or null when none is open.
- `preview`: every non-empty accessible name and text inside the open preview
  drawer in tree order, or null when it is closed.
- `field`: the focused editable field as
  `{"name","text","caret","selection","focused"}` (`selection` is
  `[start, end]` or null), or null.
- `matches`: `matches()` for the deepest pane.

The snapshot, every target, and the input log's origin belong to the browser
window the run started with, as `strata.window` is the frame the Python harness
found first; a floating chooser, which becomes the active window while it is
open, is reached through a `toplevel` target.

Final-state parity compares `view_mode`, `directory`, `panes`, `focused`,
`focused_pane`, and `dialog`, which the Python harness can read through AT-SPI
the same way; `menu`, `preview`, `field`, and `matches` are diagnostics.

The runner checks every step with the process's own parser before launching,
so a typo fails the scenario as a harness error. Setup, launch, layout,
artifact, and teardown problems, a reply that never arrives, and a process that
exits are errors too: they fail even a `known_bug` run, which only scenario
steps can satisfy.

A failed run writes `target/qa-artifacts/<scenario>/<mode>[-<variant>]/`: a
screenshot of the Xvfb root, the widget tree (headed by the window's focus,
marked when it sits on a widget that is not mapped), the command and reply
transcript, the application log, the input log, the fixture listing, the
Xvfb log, `state.json` (the `app` snapshot after the last step, taken once two
reads 250 ms apart agree, at most five seconds), and `result.json` (the run's
twin, errors, and each failed step with its line).

### Driving a session by hand

`scripts/qa-drive.sh` keeps one `strata --qa` session open in the pinned
image, for checking a change interactively before (or instead of) writing a
scenario. `start` starts a detached container with the same engine, image,
mounts, account files, and `target/e2e-container` cache as `scripts/qa.sh`
(both take them from `scripts/lib/qa-container.sh`), makes the
build `qa.sh` makes (`cargo test --features qa --test qa --no-run`), seeds a
fresh HOME, XDG directories, and fixture from `tests/fixtures/harness.json` as
the runner does, starts Xvfb on `:90`, and launches the application in the
fixture with the runner's environment; it prints the launch reply. A start
that fails at any point removes its container. Each later subcommand is a
short `docker exec` (or `podman exec`) that returns at once:

```bash
./scripts/qa-drive.sh start --tree '{"notes":{"plan.txt":"plan\n"}}' --pref browser_mode=list
./scripts/qa-drive.sh send '{"select":{"entry":"notes"}}' '{"query":"state"}'
./scripts/qa-drive.sh screenshot menu.png      # target/qa-drive/menu.png
./scripts/qa-drive.sh restart --pref text_size=large
./scripts/qa-drive.sh exec 'ls -A; cat "$XDG_CONFIG_HOME/strata/settings.toml"'
./scripts/qa-drive.sh log
./scripts/qa-drive.sh stop                      # removes the container
```

`send` writes each process command on the application's stdin and prints its
one-line reply, waiting 90 s, or the command's `timeout_ms` plus 30 s; it
reports a process that exited with its log tail. Replies are matched to
commands by number, so the reply to a command whose wait gave up is skipped
when it comes, never printed as a later command's. Runner steps (`fs`, `layout`,
`screenshot`, `restart`, and the others marked "runner" above) are not process
commands: use the subcommands, or a scenario. `--tree` takes a tree as the
manifest writes one (a name, or JSON, or `@file`), `--pref KEY=VALUE` changes a
seeded preference (the value is JSON, or else a string), `--env NAME=VALUE`
adds a variable, and `--bus` starts the runner's private session bus in place
of the address that cannot connect. `exec` runs a shell command in the
fixture with the session's HOME, XDG directories, display, runtime directory,
and bus. `restart` closes the application's stdin, signals its group if it
has not quit within ten seconds (the runner's `EXIT_TIMEOUT` and
`TERMINATE_GRACE`), rewrites the given
preferences in its saved `settings.toml`, and launches it again on the same
fixture and HOME. Every exchange is appended to
`target/qa-drive/transcript.jsonl` in the artifacts' transcript format, and the
input log, build log, screenshots, and (after `stop`) application log land in
`target/qa-drive/`, which `start` empties. There is one session per checkout,
and it shares the build cache, so never run it while `scripts/qa.sh` runs.
The values it copies from `tests/qa.rs` (the QA environment, XDG layout,
Xvfb arguments, bus configuration, and timeouts) are checked by
`scripts/test_qa_drive.py`.

### Input log

With `STRATA_INPUT_LOG=<path>` the process appends every synthetic event it
sends, one per line; the runner sets it for each run and saves the file as
`input.log` whenever it writes artifacts. Coordinates are X root coordinates minus
the main window's X surface origin, so they are relative to the window:

```text
key <keycode> down|up
motion <x> <y>
pointer <x> <y> button <n> down|up
```

`pointer` is a button event at the pointer's position, with the wheel as
buttons 4 and 5, and keycodes are the X server's. `motion` lines record each
XTEST motion, including both steps of the approach and every drag point, and
are informational only. Input parity compares just the `key` and `pointer`
lines, in order, with pointer coordinates allowed one pixel of difference; the
position a `pointer` line carries is the final pointer position before that
button event, which is what parity checks of motion. A twin harness writes the
same `key` and `pointer` lines and may write `motion` lines as it likes.

The Python harness writes the same lines from `harness/xtest.py`; the
helpers live in `harness/parity.py`, which the portal scenarios, having no
`strata` fixture, also use from `harness/portal.py`: their input is logged
relative to the chooser and the chooser's state is dumped before the request
ends (an empty snapshot once it has closed). Under pytest,
`STRATA_INPUT_LOG` names a directory, and each scenario appends to
`<directory>/<node id>.log`, with every character outside `A-Za-z0-9._-`
replaced by `_` and a name longer than 200 characters cut to its first 180
and `-` plus 16 hex digits of the node id's SHA-256. Positions are relative to the main window's origin as AT-SPI
reports it, read for each line. `STRATA_STATE_DUMP`, also a directory, makes each
scenario write `<node id>.json` when it ends: its node id and the compared
snapshot keys (`view_mode`, `directory`, `panes`, `focused`, `focused_pane`,
`dialog`) read through AT-SPI once two reads 250 ms apart agree (at most five
seconds), as the QA runner takes its `state.json`. `scripts/e2e.sh` passes both
variables into the container; they must name directories inside the checkout.
With `STRATA_PARITY_FIXTURES=1`, which `scripts/e2e.sh` and `scripts/qa.sh` also
pass through, both harnesses name each fixture directory
`/tmp/strata-fixture-<first 12 hex digits of the SHA-256 of the twin's node id>`
instead of a random name, and the directory holding HOME and the XDG
directories `/tmp/strata-home-<the same digits>`, so a typed fixture or home
path and the root pane's name are the same on both sides.

### QA parity

`scripts/qa-parity.sh` decides whether the QA scenarios and their Python twins
agree. It runs only `scripts/qa.sh` and `scripts/e2e.sh`, one after the other
because they share `target/e2e-container`, and never a host binary:

```bash
./scripts/qa-parity.sh                       # every scenario pair
./scripts/qa-parity.sh clipboard             # the pair a mutation belongs to
./scripts/qa-parity.sh two-clicks-open       # a pair by its scenario's name
./scripts/qa-parity.sh --baseline            # every pair, unmodified checks only
python3 scripts/qa_parity.py baseline NAME   # the same as --baseline NAME
python3 scripts/qa_parity.py report          # compare the saved runs again, no rebuild
```

Both runners get `STRATA_PARITY_FIXTURES=1`, the QA runs keep their artifacts,
and the Python runs write input logs and state dumps. Before any run starts,
`tests/qa.rs` prints `RUN <scenario>/<run> (twin <node id>)` for each planned run
(its harness captures no output). The runs map to pytest node ids through those
lines and the `(twin ...)` on the `PASS` and failed lines (a Python node id that
pytest-xdist's `loadgroup` suffixed with `@<group>`, as the visual baselines'
`@visual-baselines`, pairs by the bare id, and its input log and state dump are
read under the suffixed one); a planned run with no
result line, a `KNOWN BUG` result, a missing `result.json`, or a `qa.sh` exit
status its results do not account for is a disagreement. A scenario with a
`mutation` is held to all five checks; one without, or any scenario under
`--baseline`, to baseline parity: pass, input, and state. A scenario without a
`twin` has nothing to compare: parity never runs it, and the summary lists it as `No twin, not compared: <scenario>`, so naming
only such scenarios runs nothing.

| Check | Holds when |
| --- | --- |
| pass | both sides pass on the unmodified tree |
| fail | under the scenario's `mutation` patch each run fails on both sides or on neither, at least one fails on both, and neither side has an infrastructure error |
| signature | where both fail, the Python wait message (or assertion message) equals the `message` of the QA step that failed; an empty message on either side disagrees |
| input | the unmodified runs' `key` and `pointer` lines agree in order, positions within one pixel (bar the positions `input_parity_ignore` names), or, in a run that only moves the pointer, its `motion` lines do; any other line disagrees, and so do two empty logs unless no step of the scenario sends input |
| state | the unmodified runs' final snapshots both have the compared keys and agree on them, less the scenario's `state_parity_ignore` |

Everything lands in `target/qa-parity`: `baseline/` and `mutations/<patch>/`
hold each side's log, the QA artifacts, and the Python JUnit report, input logs,
and state dumps; `<unit>.md`, named after the scenario's patch or else the
scenario, tabulates one pair, mapping each run's Python wait message to the QA
step's message and line; `summary.md` has one row per pair, with `n/a` for
checks baseline parity leaves out.
Both are deleted when a run starts, and a run that cannot finish leaves a
`summary.md` saying why.
The script exits 0 only when every pair agrees, 1 when any disagrees, 2 on a
usage error or a patch that cannot be applied or restored, and 130 when interrupted. A patch whose files have uncommitted
changes is refused, and only the patched files are restored afterwards, so the
script runs unattended.

While both suites exist, a behaviour change updates a Python test and its QA
twin together, including any step `message` that mirrors a `strata.wait`
message.
CI does not run parity; run `./scripts/qa-parity.sh` locally when a pair changes.

### Python-only tests

Every collected Python scenario has a QA twin, is listed here, or is the twin
of a skipped scenario (see "Skipped scenarios"). A test stays Python-only only
for a concrete technical reason, such as checking the AT-SPI surface itself,
and is listed here with that reason.

- `test_quick_preview.py::test_large_table_header_sort_reaches_rows_beyond_old_limits`
  (all four tables), unverified and pending an owner decision: every step
  ports and passes up to the last, and the two harnesses send identical input
  logs, yet the last Ctrl+C, after an autoscrolled cell selection and a click
  on the preview header, copies the whole table in `--qa` mode and nothing
  under the Python harness. In `--qa` mode keyboard focus stays on the cell
  label the drag started on, which the list has scrolled away and unmapped,
  so the key reaches the table's own copy-everything handler. A pause, a
  longer autoscroll, no second display connection, and a run with GTK's
  AT-SPI bridge registered on a private bus change nothing; through AT-SPI
  the twin sees no focused node at all before that Ctrl+C. The cause of the
  difference is not established.

### Skipped scenarios

A scenario whose pass or fail the app leaves to chance under both harnesses,
so neither `known_bug` nor a pass can be relied on, carries a `skip` reason
until the app is fixed; it stays in the suite and runs when named.

- `settings-text-size-keeps-switches` (twin
  `test_text_size.py::test_settings_text_size_keeps_switches_inside_the_page`):
  after the Settings window shrinks from 1000 to 640 pixels at 32-pixel text,
  "Complete setup" can stay four pixels below the General page's viewport with
  no scroll left to reveal it. The twin failed 24 of 24 runs with its input
  logged and passed four of four without; the QA scenario passed and failed by
  turns in full runs.

### QA-only waits

A QA run sends input faster than its twin, whose AT-SPI lookups take tens of
milliseconds. Where that pace lets the app's own timers fire in another order
than they always do under the twin, the QA scenario waits for what the twin's
pace guarantees, and is listed here:

- `same-folder-copy-numbered-duplicate` (folder): waits for the documents
  column the keyboard cursor mirrors after 75 ms before Ctrl+D. Over 24
  sequential twin runs the duplicate always ends selected in the root column
  alone.
- `paste-targets-selected-directory` (Columns): waits for the archive column
  the cursor mirrors before Ctrl+V. Over 24 twin runs the pasted file always
  ends selected and focused in that column.

- `escape-preserves-name-remaining` (existing-folder),
  `valid-name-commits-remaining` (enter- and sidebar-existing-folder in
  Columns), `rename-and-undo-restores` (folder), and
  `repeated-renames-select-stem` (folder in Columns): wait for the folder's
  column the cursor mirrors before F2. Over 24 sequential twin runs each
  ends in the same state every time, with that column open beside the folder
  wherever the twin stays in its parent (the sidebar case ends in Home).

These expose an app race: Ctrl+D, Ctrl+V, or F2 within 75 ms of the arrow key
that moved the cursor onto a folder leaves a stale mirrored column, and a paste
then leaves the pasted file neither selected nor focused.

- `rename-and-undo-restores`: settles the renamed row before Ctrl+Z. The
  directory monitor can list the new name, and the listing select and focus
  it, before the rename's own completion publishes its undo and ends the
  operation; a Ctrl+Z in between is dropped. The app shows nothing a run can
  read to tell that the undo is ready (Undo has no menu item or action
  state), so the wait is the settle. Under the same parallel load, 3 of 60
  QA runs without it lost the Ctrl+Z and none of 60 with it did (6 of 116
  and 0 of 84 over every probe). Over 24 twin runs of both kinds Ctrl+Z
  always restored the original.
- `committed-rename-stays-visible`, `airy-committed-rename-visible`,
  `visible-rename-preserves-scroll`, `click-away-rename-respects-navigation`,
  and `visible-created-item-preserves-scroll`: wait for F5's reload to replace
  the `rename-target` row before opening it. The row is remembered after it
  is listed and just before F5; in 48 probe runs that waited 1.5 s instead of
  pressing F5, the row was never replaced, so the replacement the wait sees
  is F5's. The app cancels a click on a row the listing replaces mid-press;
  QA's pace reaches that window, and over 24 sequential runs of every case
  the twins' never did.
- `new-folder-with-selection-groups` (Columns): waits for the renamed
  `grouped` row to count its two items before clicking it. The row first
  shows the optimistic rename, and a click on it before the listing catches
  up leaves focus in the parent column (16 of 24 QA runs without the wait,
  none of 36 with it); over 24 twin runs the click always focused the
  folder's column.
- `tenxer-d-without-trash-confirms`: waits for the `no-trash` link to be
  listed before walking the cursor onto it, which over 24 twin runs the
  fixture's own setup time always allowed.
- `narrow-window-keeps-last-column`: settles the `folder` column before
  comparing its right edge with the preview's. Once the 900-pixel window first
  fits the column, the column strip scrolls it clear of the preview a moment
  later; QA's pace compares in between (the column ended at 802 against the
  preview's 600, then at 600). Over 24 sequential twin runs the comparison
  always held.
- `empty-directory-focus-and-tab-order` (pointer): once the new `empty` row
  is listed after F5, waits up to a second for the listing to replace that
  row once more (it always did within a second over 36 probe runs) before
  clicking it; a click on the row the listing replaces mid-press is
  cancelled, and 4 of 12 QA runs without the wait lost it. Over 24 sequential
  twin runs of each pointer case the click always opened `empty`.
- `invalid-archive-reports-damage`: waits for the error detail label once the
  progress dialog has turned into `Unable to complete operation`, before
  asserting it; the label can show a moment after the dialog takes its new name,
  and one QA run in about fifty checked in between. Over 24 sequential twin
  runs of all seven cases the assertion always held at once.
- `preview-mode-survives-unsupported` (Columns): waits for the Beta column the
  cursor mirrors before pressing Space on Beta, as the same-folder copy waits
  above do. Without it Space enters Beta before the mirror and the app never
  mirrors Beta's cursor into a Gamma column, which the twin's final state
  always shows (24 sequential runs).

### Nondeterministic final state

`startup-with-closed-pipes` sets `state_parity_ignore` to `["*"]`: the twin
starts the application itself with its output pipes closed and terminates
that process in its `finally:`, so the `strata` fixture's state dump finds no
application on the accessibility bus and records an error instead of a state.
The QA run's own final state is still kept in `state.json`.

`sidebar-destinations-accept-three-files` sets `state_parity_ignore` to
`["focused","panes.focused"]`: after three selected files are dropped on Home,
the app moves focus to `todo.txt` or `readme.md` depending on how the removals
arrive. Over 24 sequential twin runs of `[Home-columns]` it ended on
`todo.txt` 16 times and `readme.md` 8 times; QA runs vary alike. The test
asserts no focus, and the rest of the state is still compared.

### Nondeterministic input

`png-conversion-consent-reuses-download` and `png-conversion-errors-keep-chooser`
set `input_parity_ignore` to `key down`/`key up` for the ten key events that
type the image server's port into the chooser's name field (events 45 to 54,
and 127 to 136 for the second submission): both twins serve the image on an
ephemeral port (`ThreadingHTTPServer(("127.0.0.1", 0), ...)`), always five
digits, so the same keys stand there with other digits. Every other key of the
URL, and every other event, is compared.

`zipcrypto-collision-reopens` sets `typed digits` at event 11 in both runs:
the twins write the archive with a random 11-byte header, so the colliding
password each side finds and types is a different number, of one to four
digits.

`pdf-zoom-keeps-text-size` sets `input_parity_ignore` for its `columns-wheel`
run's events 19 (`button 1 down`) and 20 (`button 1 up`), the click on the PDF
page after Ctrl+wheel zoomed it.
The Ctrl+wheel handler resizes the pages, then `preserve_pdf_view_center`
moves the scroll adjustments in a default-priority idle: run before GTK
allocates the enlarged pages, the adjustments clamp to the old range and the
page keeps its origin (centre 968,416); run after, the view centre is kept
and the page lands at 1035,387. The page's size is the same either way. Over
24 sequential runs the twin clicked 968,416 22 times and 1035,387 twice; the
QA run, 13 and 11 times. Both always pass; every other event is compared.

## In CI

The E2E suite does not run on pull requests or pushes. Run it on demand from
**Actions → E2E → Run workflow**, choosing the branch to test, or with
`gh workflow run e2e.yml --ref <branch>`. Enable
`require_published_environments` to bypass the fast dependency cache and require
verified published base images.

### Three-minute critical path

`.github/workflows/e2e.yml` has three E2E stages:

1. **E2E build and plan** restores BuildKit layers for the pinned native/Python
   dependencies, Rust toolchain, and compiled `Cargo.lock` dependencies. It
   compiles the tested revision **once**, without debug information or incremental
   artifacts, and collects the real pytest inventory (including parameter IDs).
   It caches a Zstandard-compressed runtime archive by rendering inputs and UID/GID,
   independently of source revisions, and uploads the binary, checksummed provenance,
   and complete shard plan. A separate small plan artifact keeps aggregation small.
2. **E2E shard N** jobs start on independent 4-vCPU runners, restore that exact runtime
   archive, download the attempt's binary bundle, and invoke `./scripts/e2e.sh` with two isolated
   xdist workers. Shards do not start BuildKit, contact a registry, restore Cargo
   caches, compile, or install packages. Every runner uses
   the same pinned rendering packages and baselines as a local canonical run.
3. **End-to-end GUI suite** aggregates the run. It requires
   every dependency to succeed, verifies that all planned node IDs passed setup,
   call, and teardown exactly once. Timing is **informational**, with a three-minute
   performance target—not a pass/fail condition. From build-job creation through
   aggregation, the initial E2E queue, setup,
   dependency installation or cache retrieval, compilation, transfers, downstream
   runner queues, and test execution are included—not just pytest time. Queue and
   execution durations appear in the Actions summary. Slow runs and unavailable
   timing telemetry do not fail passing tests. Final teardown cannot be measured
   from inside its own job;
   use the Actions completion timestamp to verify the final observed runtime.

The matrix is generated from `harness/sharding.py`, not a fixed runner count or
file list. Tests are scheduled longest-first using committed setup+call+teardown
CI measurements from `tests/e2e/durations.json`, with 25% headroom and a 180-second
soft target per worker (two workers per runner). This leaves room for runner setup
within an approximately three-to-four-minute shard job; actual timings may vary.
New tests automatically receive a conservative five-second weight. More tests or longer measured durations add runners
up to a maximum of three shards, reducing duplicated runtime setup and bounding
fan-out. At the cap, shards run longer rather than failing planning or dropping tests.
Baselines stay in one serial scheduling group, even when that group exceeds the soft
target. Estimated time alone never fails the gate; hang-protection timeouts still apply.
Tune `TARGET_SECONDS` and `MAX_SHARDS` in `tests/e2e/harness/sharding.py` manually as
runtime and cost needs change. The shard cap is not a spending cap: longer runs still
consume more runner-minutes. There is no `max-parallel` throttle; the runner provider
must have enough concurrent capacity for up to three shards. Runner queues affect
reported timing, not test correctness.

Shards validate their entire collection against the plan before selecting tests.
A missing, extra, skipped, failed, or stale result fails the aggregate gate.
`fail-fast: false` preserves other shards' diagnostics. Worker crashes and failed
assertions are not retried; only display infrastructure startup retains its one
retry. CI caps each test at 60 seconds and each shard job at ten minutes to stop
hung execution, independently of the informational three-minute overall target.
Reports and JUnit files are uploaded on both success and failure, with distinct
artifact names per runner and run attempt; screenshots/trees/logs are uploaded on
failures. The gate never mixes previous attempts into a fresh measurement.

### Understanding a failed gate

The prerequisite check reports whether bootstrap or shard execution failed, links
unsuccessful jobs and steps, and inspects a bounded amount of their logs. Confirmed
Arch Linux Archive HTTP errors, compiler diagnostics, and provenance failures are
identified separately. When bootstrap fails, it explicitly says that no scenarios
ran. Timing is reported separately and never changes the test result. If logs
cannot be retrieved or classified, it says so rather than guessing the cause.

### QA in CI

One job in `ci.yml` runs the QA runner on non-draft pull requests and manual
dispatches. It is **informational**: it has `continue-on-error: true`, no
required check needs it, and the repository ruleset does not list it, so a red
QA job never blocks a merge. CI does not run QA parity.

**QA scenarios** waits for **Quality build and lint**, pulls (or loads) the
same pinned build environment the Rust shards use, restores main's quality
Cargo cache, and runs `./scripts/qa.sh` with
`STRATA_E2E_CARGO_CACHE=target/quality-container`. `--features qa` equals
`--all-features`, so that cache already holds every dependency built in the
same image at the same paths and only `strata` and the runner compile. The E2E
runtime image and binary bundle cannot be reused: the runtime has no Rust
toolchain, and the bundled binary lacks the `qa` feature. The job then checks
the `qa` build contains `usage: strata --qa`, the prefix of `USAGE` in
`src/qa.rs` that the release workflow rejects in release binaries;
`scripts/test_qa_parity.py` requires both checks' pattern to prefix `USAGE`. A
failed run uploads `target/qa-artifacts` as `qa-artifacts-<attempt>`.

`scripts/qa.sh` and `scripts/e2e.sh` accept `STRATA_E2E_CARGO_CACHE`, a
directory under `target/` holding `cargo/` and `build/` (default
`target/e2e-container`); leave it unset locally. To read a failure, start with
the job log (`PASS` and failed lines name each run's twin), then the artifacts:
`qa-artifacts-<attempt>` has the per-run bundle described in "Replies and the
state snapshot".

**Promotion is pending.** The owner makes **QA scenarios** a required check
after a two-week informational window with zero unexplained failures and a
documented wall time per run, recorded in the promoting pull request; that
change removes its `continue-on-error` and adds it to the ruleset's required
checks. No informational window has been recorded yet.

### Cache lifecycle and cold starts

BuildKit's content-addressed cache invalidates on the actual Dockerfile, package
installer, GTK patch/probe, requirements, Rust manifest/lockfile, source, and
resource inputs. `install-packages.sh` configures only the dated HTTPS Arch Linux
Archive mirror and requires Pacman's package-signature policy. It refreshes the
signed `core` and `extra` databases once, then performs a full snapshot upgrade
and requested package installation as one transaction; moving repositories and
partial upgrades are never mixed into the image.

The `package-indexes` stage fetches the dated Pacman databases once. Runtime,
patched-GTK, and toolchain stages inherit those exact indexes; none refreshes from
a moving mirror. The warmer publishes the index-stage cache **before** package,
GTK, and compiler bootstrap, so a later failure does not discard a successful
index fetch. Source builds attach the public input digest as an output image label,
not an environment variable or secret-looking build argument.

A stub application
warms dependencies only; its executable and all Strata fingerprints are removed
before the real source is copied and compiled. The bundle is tied to the checked-out
commit, source/resource contents (including local edits), rendering inputs, binary
checksum, and plan checksum. The runtime image's input label must match too; it cannot be replaced
by an arbitrary host binary or an artifact from another run.

`Warm E2E dependencies` publishes the full dependency cache in a separate workflow,
with a 30-minute bootstrap allowance: daily, on image/dependency changes on main,
and on manual dispatch. Dispatch it on a branch to warm that branch's cache.
The timed build restores a BuildKit local-cache directory through the cache action's
segmented transfer path, keyed by image inputs, manifests, ignore rules, and UID/GID.
BuildKit still validates content-addressed dependency records before reusing them.
A missing local cache first looks for published environments, then falls back to
the shared GHA dependency cache; the old
`strata-e2e-v1` cache remains a read-only migration source. The warmer publishes both
formats and replaces its local export directory so obsolete blobs do not accumulate.

The timed build does not publish an application BuildKit cache. It compiles the
tested revision against cached dependencies and immediately hands off the binary
and plan instead of waiting for cache export. Repeated same-revision runs therefore
exercise the same compilation path as new revisions, not a misleading binary-cache
shortcut.

The runtime archive uses a separate exact-key cache; shards restore it without
starting BuildKit. Only the producer exports or saves a missing archive. It confirms
that publication succeeded before telling shards to use the cache. If publication
is unavailable (including read-only fork caches), it instead uploads an explicit
attempt-scoped `e2e-runtime-<attempt>` artifact; transfer time remains in the timing report.
This avoids repeatedly uploading and unzipping a large unchanged runtime on ordinary
source revisions without making cache-write permission a prerequisite for testing.

Node-24-native actions verify artifact download digests; the runner also checks
bundle provenance and the loaded image's input label. GitHub's cache branch scoping
allows fork PRs to read the main cache without credentials or registry access and
prevents PR caches from replacing main's cache. No `pull_request_target` execution
or package-write permission is needed. Main cannot restore a PR-scoped cache:
a successful PR run does not seed main's first bootstrap after merging a new
workflow. Run the trusted default-branch warmer to populate main's own caches;
do not promote untrusted PR build caches into main. These caches are evictable,
not permanent package storage. Published environments provide the durable fallback
below; upstream outages can still block the first publication of new inputs.

### Persistent published environments

`Publish pinned E2E environments` publishes two GHCR packages from trusted main:

- `ghcr.io/lgse/strata-e2e-runtime`: the pinned GUI/Python/font environment.
- `ghcr.io/lgse/strata-e2e-build`: Rust plus the runtime and compiled locked Cargo
  dependencies. The stub application and its fingerprints are removed; no tested
  application binary is published here.

Tags include SHA256 input identifiers, architecture, UID, and GID. Runtime inputs
are independent of application source; the build identifier additionally includes
`Cargo.toml`, `Cargo.lock`, and `.dockerignore`. Publication runs on input changes
or manual dispatch, not ordinary application commits. Existing publications are
checked before rebuilding. A separate `base-<environment-inputs>-linux-amd64-1001-1001`
tag in the build package provides a reusable local environment independent of
application manifests. It is published once per environment input set and is not
retagged on ordinary Cargo changes. GHCR storage is independent of Actions cache eviction;
retain tags needed by supported revisions rather than treating them as disposable
per-run artifacts.

On a fast dependency-cache miss, CI resolves matching image tags to **OCI digests**,
checks their platform and input labels, and uses them directly as application and
runtime bases. Published labels are preserved, not overwritten to match a checkout. Those builds do not
execute Arch/Python/Rust bootstrap stages, even with an empty BuildKit cache.
The application still compiles for the tested revision, and the base's Cargo
manifests must match the checkout. A fresh runner still transfers image layers;
this avoids package-server requests and dependency compilation, not all network I/O.

The build package also stores complete BuildKit caches. An environment-only cache
identifier lets new Cargo inputs reuse unchanged GTK/Rust dependencies. The warmer
can import these persistent caches to refill the faster segmented GitHub caches.
If an image is absent or inaccessible, CI reports that fact and falls back to
verified cache/source building; it never substitutes `latest`, another dependency
version, or an unverified bundle.

Only the main-only publisher has `packages: write` and registry login credentials.
PR CI and warmers pull anonymously and cannot publish shared images. After the
first publication, **make both packages public in GHCR package settings** and rerun
the publisher. Its anonymous-access check fails descriptively until they are
readable without credentials. Do not give fork PRs package-write credentials.

To exercise the registry path, manually dispatch **CI** with
`require_published_environments` enabled. This bypasses the fast dependency cache
and fails if matching public bases are unavailable; it cannot silently benchmark a
source-build fallback. All scenarios and the same informational timing report still apply. The
runtime archive cache remains enabled, so this is not a completely cold transport
benchmark.

New environment inputs still require an initial trusted publication; a PR cannot
seed main by publishing its own build. The three-minute target is informational
for both warm and cold runs.

**New inputs without published images or cached bootstrap are not guaranteed to
install and compile within three minutes.** The timed build retains a ten-minute
limit to stop hung bootstrap jobs; elapsed time alone never fails the aggregate.
Cold and warm durations are reported honestly rather than conflated. Large
cache publication no longer blocks binary handoff or races that ten-minute limit;
the separate warmer owns it. Seed the dependency cache before enabling the new
required gate, and rerun the **entire workflow** after the warmer completes. External package outages, cache eviction, and runner queues cannot
be solved by adding test shards. Inspect the build logs' `CACHED` entries and the
critical-path summary rather than raising the time limit.

### Measured fresh-revision run

[CI run 34235908938](https://github.com/lgse/strata/actions/runs/34235908938)
(`84d8e82`, 2026-09-08) compiled Strata again and passed all 581 cases exactly once
on 30 runners in **172 seconds**, measured from the initial E2E queue/attempt start
through the aggregate job's completed timestamp. The internal measurement was
168.7 seconds before teardown. The runtime archive was cached; the new segmented
dependency cache missed, so this run exercised the GHA dependency-cache fallback
while the separate warmer published the new format. This was not an identical
binary-cache rerun or a completely cold package bootstrap.

Attempt 2 of the same run restored both segmented caches and **recompiled Strata
in 7.51 seconds**. All 581 cases passed exactly once: **139 seconds** from initial
E2E queue to completion, **141 seconds** from attempt start, and 137.4 seconds at
the internal measurement. This is same-revision recompilation, not a second new
revision or a binary-cache shortcut. These measurements predate persistent GHCR
base images; they are not a benchmark of the registry-only cold-runner path.

### Reproducing and maintaining shards

To collect the current inventory inside the canonical container:

```bash
./scripts/e2e.sh -n 0 --collect-only --e2e-write-plan=target/e2e-plan.json
./scripts/e2e.sh --e2e-plan=target/e2e-plan.json --e2e-shard=0 \
  --e2e-report=target/e2e-reports/shard-0.json
```

For the exact CI binary, use a disposable checkout at the commit in its metadata
(the head of the dispatched branch). Download `e2e-bundle-<attempt>` into
`target/e2e-bundle`, build the `runtime` target with the invoking UID/GID, and run
(use `docker` instead of `podman` if appropriate):

```bash
podman build --target runtime --tag strata-e2e:ci-runtime \
  --build-arg E2E_UID="$(id -u)" --build-arg E2E_GID="$(id -g)" \
  --label org.strata.e2e.inputs="$(python3 scripts/e2e_bundle.py image-key)" \
  --file tests/e2e/Dockerfile .
chmod +x target/e2e-bundle/strata
STRATA_CONTAINER_ENGINE=podman STRATA_E2E_IMAGE=strata-e2e:ci-runtime \
  STRATA_E2E_BUNDLE=target/e2e-bundle ./scripts/e2e.sh --e2e-plan=target/e2e-bundle/plan.json --e2e-shard=0
```

The bundle must be inside the checkout. Without these explicit bundle/image
variables the local canonical runner reuses its verified base and compiles the
checked-out application normally; it does not rebuild the base.

After a complete CI attempt with every test passing, download its `e2e-report-<attempt>-*` artifacts into an empty
`target/e2e-reports` directory and its `e2e-plan-<attempt>` into
`target/e2e-bundle`, then refresh timings:

```bash
python3 scripts/e2e_ci.py durations target/e2e-bundle/plan.json target/e2e-reports \
  > tests/e2e/durations.json
```

Review and commit the changes. Reports must cover every test and shard; partial or
failed runs cannot overwrite scheduling measurements. Durations are scheduling
hints, never an allowlist: new tests always participate without editing this file.

### Maintaining behavioral coverage

Before consolidating tests, identify the retained behavioral owner for every
assertion. Fewer test functions or collected cases do not establish a runtime
improvement. Preserve functional layout, input-routing, filesystem-safety,
live-preference and lifecycle regressions, even when they share fixtures.
Record task-specific inventories and consolidation decisions in the issue or PR,
not in a committed audit report.

### Coverage audit (#607)

154 GUI cases were removed from the 729-test inventory (six new harness tests
exercise collection/sharding/reporting). No workflow was moved into an optional,
nightly-only, or changed-files-only suite.

| Removed/reduced coverage | Retained owner |
| --- | --- |
| Six invalid strings × mode × kind × new/existing × completion, reduced to `bad/name` (120 cases removed) | `src/services/operations/tests.rs::basenames_reject_empty_reserved_nested_absolute_and_nul_names` (no GTK/display requirement); GUI keeps one Enter and one real click-away |
| Four invalid names in correction/reopen workflow, reduced to one (9) | Same validation tests; correction/reopen still runs in all three views |
| Four accepted-name variants in inside-field click workflow, reduced to ` padded ` (18) | `basenames_accept_single_native_and_unicode_components`; GUI still checks exact untrimmed names for files and folders |
| Standalone new-folder Escape and existing-file cancellation tests (4) | `test_inline_renaming.py::test_escape_preserves_the_original_name`, covering both lifecycles and kinds |
| Standalone invalid rename in dialogs suite (1) | Stronger synchronized `test_invalid_names_retain_the_original`, including filesystem contents and GTK-critical checks |
| Separate preview metadata/list-preservation launches (2) | Assertions consolidated into `test_space_opens_and_closes_the_quick_preview` in all three views |

All six valid-rename dismiss targets remain as real XTEST focus walks: GTK's in-flight
focus walk is not covered by emitting a controller signal in Rust. Enter and sidebar
still run across modes, kinds, and new vs existing items; the four shared click-away
targets run in Columns and List on an existing file. Real drag/XTEST routing,
caret visibility, clipboard selection/undo, multi-window preferences, accessibility
semantics, and all six visual baselines also remain. The removed validation vectors
are covered by unconditional, display-independent Rust tests—not by tests that
silently return when GTK cannot initialize.

Before the shared environment moved to Arch, enabling all GTK fixtures in the
former Ubuntu quality job revealed a pre-existing offscreen new-entry failure
tracked in #613. This environment update does not delete that fixture, change
application scrolling, or hide it behind a new skip. Continue running Rust GTK
tests on a private display locally as described above.

The existing copy-conflict/undo scenario also waits for dialog dismissal and the
copied entry's keyboard focus before sending Ctrl+Z. A selected-state notification
alone was too early for this follow-up keyboard action; the filesystem/undo assertions
remain unchanged. There are no assertion retries or fixed sleeps.
