# SPDX-License-Identifier: MIT
"""Empty-space clicks clear selection without taking over item clicks or drags."""

import subprocess

import pytest
from PIL import Image

from harness.modes import ALL_MODES


def _select_files(strata):
    root = strata.fixture.root.name
    strata.select_entry("readme.md", root)
    strata.click_entry_with("todo.txt", ["ctrl"], root)
    strata.wait_for_selection(["readme.md", "todo.txt"], root)
    return root


def _blank_point(strata, directory):
    bounds = strata.pane(directory).screen_bounds()
    return bounds.center[0], bounds.y + bounds.height - 60


@pytest.mark.parametrize("mode", ALL_MODES)
def test_background_click_clears_all_selection(strata, mode):
    root = _select_files(strata)
    strata.pointer.click(strata.pane(root), at=_blank_point(strata, root))
    strata.wait(lambda: not strata.all_selected_names(), "empty space to deselect every file")
    assert strata.pane().name == root


@pytest.mark.parametrize("mode", ALL_MODES)
def test_background_press_clears_selection_before_drag_intent(strata, mode):
    root = _select_files(strata)
    start = _blank_point(strata, root)
    end = strata.entry("todo.txt", root).screen_bounds().center

    def cleared_on_press():
        assert strata.selected_names(root) == []

    strata.pointer.drag_points(start, end, after_press=cleared_on_press)
    strata.wait(lambda: bool(strata.selected_names(root)), "marquee selection to survive release")


@pytest.mark.usefixtures("unreserved_columns")
def test_clicking_an_empty_column_clears_other_columns_without_closing_them(strata):
    strata.open_directory("archive")
    _select_files(strata)
    panes = strata.pane_names()
    strata.pointer.click(strata.pane("archive"), at=_blank_point(strata, "archive"))
    strata.wait(lambda: not strata.all_selected_names(), "empty column to clear all selections")
    assert strata.pane_names() == panes
    assert strata.current_directory() == "archive"


def test_clicking_beside_the_last_column_clears_selection(strata):
    root = _select_files(strata)
    window = strata.window.screen_bounds()
    pane = strata.pane(root).screen_bounds()
    point = (window.x + window.width - 40, pane.center[1])
    strata.pointer.click(strata.window, at=point)
    strata.wait(lambda: not strata.all_selected_names(), "blank space beside Columns to deselect")


def _write_photo(strata):
    Image.new("RGB", (1600, 900), "green").save(strata.fixture.path("photo.png"))


def _blank_preview_point(strata):
    preview = strata.preview()
    bounds = preview.screen_bounds()
    point = (bounds.center[0], bounds.y + bounds.height - 30)
    for node in preview.find_all():
        if node is preview:
            continue
        box = node.screen_bounds()
        inside = box.x <= point[0] < box.x + box.width and box.y <= point[1] < box.y + box.height
        assert not inside or node.role in ("panel", "filler", "group"), (
            f"the blank preview point lands on {node!r}"
        )
    return point


@pytest.mark.preferences(single_click_previews=True)
@pytest.mark.parametrize("mode", ALL_MODES)
def test_preview_blank_click_keeps_the_selection_and_a_drag_still_selects(strata, mode):
    _write_photo(strata)
    root = strata.fixture.root.name
    strata.wait(lambda: strata.entry("photo.png", root), "the generated photo")
    strata.select_entry("photo.png", root)
    strata.wait(lambda: strata.preview_shows("photo.png"), "the image preview")
    point = _blank_preview_point(strata)
    strata.pointer.click(strata.preview(), at=point)
    strata.settle(strata.pane(root))
    assert strata.selected_names(root) == ["photo.png"]
    assert strata.preview_shows("photo.png")
    if mode == "Columns":
        assert strata.pane().name == root
    strata.pointer.drag_points(point, strata.entry("readme.md", root).screen_bounds().center)
    strata.wait(
        lambda: "readme.md" in strata.selected_names(root)
        and "photo.png" not in strata.selected_names(root),
        "a plain marquee from the preview to replace the selection",
    )


def _write_sound(fixture):
    subprocess.run(
        [
            "ffmpeg", "-v", "error", "-y", "-f", "lavfi",
            "-i", "sine=frequency=440:sample_rate=44100:duration=3",
            "-c:a", "pcm_s16le", str(fixture.path("sound.wav")),
        ],
        check=True,
        timeout=30,
    )


def _write_clip(fixture):
    subprocess.run(
        [
            "ffmpeg", "-v", "error", "-y",
            "-f", "lavfi", "-i", "color=c=blue:s=320x180:r=24:d=2",
            "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000:duration=2",
            "-c:v", "libx264", "-c:a", "aac", "-ac", "2", "-shortest",
            str(fixture.path("clip.mp4")),
        ],
        check=True,
        timeout=30,
    )


@pytest.fixture
def media_file(request, fixture_tree):
    """Written before launch: a file written while Strata runs can be replaced in the listing mid-click."""

    name, write, control = request.param
    write(fixture_tree)
    return name, control


@pytest.mark.preferences(single_click_previews=True)
@pytest.mark.parametrize(
    "media_file",
    [
        ("sound.wav", _write_sound, "Next audio file"),
        ("clip.mp4", _write_clip, "Next video"),
    ],
    ids=["audio", "video"],
    indirect=True,
)
def test_disabled_transport_click_keeps_the_selection(media_file, strata):
    name, control = media_file
    root = strata.fixture.root.name
    strata.wait(lambda: strata.entry(name, root), "the generated media file")
    strata.select_entry(name, root)
    next_button = strata.wait(
        lambda: strata.preview() and strata.preview().find(role="button", name=control),
        "the media transport",
    )
    assert not next_button.has_state("sensitive")
    strata.pointer.click(next_button)
    strata.settle(strata.pane(root))
    assert strata.selected_names(root) == [name]
    button = strata.preview() and strata.preview().find(role="button", name=control)
    assert button is not None
    assert not button.has_state("sensitive")
