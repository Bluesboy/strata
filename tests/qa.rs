// SPDX-License-Identifier: MIT

//! Runs the `.jsonl` scenarios in `tests/qa/scenarios/<twin test file>/` against
//! `strata --qa`.
//!
//! Each run gets its own Xvfb, HOME, XDG directories, and fixture tree, seeded
//! from `tests/fixtures/harness.json` exactly as the Python harness seeds
//! them. App commands stream to the process; `layout` and `fs` steps run here.
//! Run through `./scripts/qa.sh`; see `docs/e2e-testing.md`.

// The process's own command parser, so a malformed step fails when the
// scenario loads, not as a scenario failure a `known_bug` could excuse.
#[path = "../src/qa/protocol.rs"]
#[expect(dead_code, reason = "the runner only validates commands")]
mod protocol;

use std::{
    collections::BTreeMap,
    fmt::Write as _,
    fs::{self, File},
    io::{BufRead, BufReader, Write},
    os::{fd::AsRawFd, unix::fs::PermissionsExt},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Mutex,
        mpsc::{self, Receiver, RecvTimeoutError},
    },
    time::{Duration, Instant, SystemTime},
};

use serde::Deserialize;
use serde_json::{Map, Value, json};
use x11rb::{
    protocol::{xkb, xproto},
    rust_connection::RustConnection,
    wrapper::ConnectionExt as _,
};

macro_rules! scenarios {
    ($($name:ident),* $(,)?) => {
        $(
            #[test]
            #[ignore = "QA scenarios run in the pinned container through ./scripts/qa.sh"]
            fn $name() {
                run_scenario(stringify!($name));
            }
        )*
        const REGISTERED: &[&str] = &[$(stringify!($name)),*];
    };
}

// One test per scenario file, named after it with underscores; names are unique
// across the folders.
scenarios! {
    action_editor_validates_saves_reopens,
    activation_without_application_empty_state,
    activation_without_default_opens_visible_app,
    airy_committed_rename_visible,
    alt_up_and_history,
    appearance_menu_switches_view,
    appearance_menu_switches_view_list,
    archive_activation_extracts_subfolder,
    archive_conflict_keyboard_choices,
    arrow_keys_move_focus,
    arrow_keys_move_focus_remaining,
    arrow_scope_toggles_live,
    background_click_clears_selection,
    background_customize_ancestor_column,
    background_customize_presented_folder,
    background_press_clears_before_drag,
    background_rename_keeps_scroll,
    batch_rename_template_confirms,
    beside_last_column_clears,
    breadcrumb_returns_to_parent,
    browser_workers_reuse_processes,
    cached_icons_fill_dimensions_after_jump,
    camera_formats_preview_sandbox,
    click_after_navigating_reanchors,
    click_away_rename_respects_navigation,
    click_mode_preference_live,
    click_then_double_click_no_rename,
    clicking_inside_keeps_new_entry,
    clicking_open_folder_focuses_column,
    column_autofit_stays_inside,
    column_background_click_focuses_parent,
    column_edge_scrolls_listing,
    column_header_titles_bounded,
    column_preview_fills_free_space,
    columns_dismissed_preview_stays_closed,
    columns_home_end_mirror,
    columns_keyboard_opens_preview,
    columns_keyboard_respects_disabled_previews,
    columns_keyboard_stays_without_mirror,
    columns_never_hover_peek,
    columns_preview_reopens_after_closing,
    columns_reclick_open_folder_renames,
    columns_return_selects_source_folder,
    columns_terminal_parent_pane_folder,
    committed_rename_stays_visible,
    compress_then_chooser_extracts,
    context_menu_copy_and_paste,
    context_menu_cut_and_paste,
    context_menu_keeps_column_target,
    control_click_toggles,
    copy_leaves_source,
    copying_directory_copies_contents,
    created_entry_completion_keeps_focus,
    created_folder_editor_narrow_window,
    cross_volume_enter_choice,
    cross_volume_plain_copy,
    ctrl_drag_copies_keeps_selection,
    ctrl_drop_on_tab_copies,
    current_breadcrumb_opens_hierarchy,
    customization_follows_move_and_trash,
    customization_follows_rename,
    customize_takes_focus_single_escape,
    cut_moves_only_after_paste,
    delete_animation_keeps_survivors,
    delete_to_trash,
    delete_trashes_filtered_result,
    delete_without_trash_focuses_cancel,
    deleting_open_folder_returns_to_parent,
    deleting_open_root_closes_children,
    directory_only_filter_matches,
    disabled_transport_click,
    dismissed_overlays_return_focus,
    dismissing_conflict_preserves_both,
    dismissing_filter_keeps_hidden,
    double_click_leaves_folder_open_focused,
    drag_cancels_folder_peek,
    drag_completion_keeps_source_label,
    drag_file_onto_folder,
    drag_from_clipped_column_narrow_window,
    drag_onto_pane_background,
    drop_file_on_itself,
    drop_folder_into_itself,
    drop_keeps_selection_at_destination,
    drop_on_parent_breadcrumb,
    drop_on_partially_visible_column,
    drop_on_tab_moves_files,
    empty_column_clears_other_columns,
    empty_name_space_drag_starts_marquee,
    enter_after_escape_opens_folder,
    entry_menu_named_actions,
    escape_clears_selection_no_navigation,
    escape_dismisses_transient,
    escape_only_clears_active_column,
    escape_preserves_name,
    escape_preserves_name_remaining,
    executable_menu_confirmed_run,
    executable_requires_confirmation,
    file_drag_does_not_open_preview,
    file_manager_reveals_requested_name,
    file_properties_describes_and_closes,
    filled_preview_outlines_minimum,
    filter_rename_shortcuts_hidden_selection,
    filter_results_follow_external_changes,
    filter_right_arrow_moves_caret,
    filter_selection_uses_theme,
    filter_updates_retain_state,
    filtered_clipboard_keeps_status,
    filtered_control_selection_group,
    filtered_item_menu_real_location,
    filtered_properties_rename_inline,
    filtered_properties_visible_selection,
    filtered_rename_nested_duplicate,
    filtered_result_to_sidebar_folder,
    filtered_result_waits_for_release,
    filtered_results_group_selection,
    filtered_thumbnail_stays_rendered,
    filtering_pane_narrows_listing,
    floating_chooser_transfers_files,
    focus_usable_after_view_change,
    folder_click_keeps_tab_name,
    folder_into_another_folder,
    global_search_activation_in_open_folder,
    global_search_activation_previews_file,
    global_search_arrows_keep_typing,
    global_search_finds_file_under_home,
    global_search_preview_follows_neighbor,
    global_search_reveal,
    global_search_reveal_visited_list,
    hidden_files_toggle,
    hovered_tab_takes_drop,
    icons_keep_layout_until_toggled,
    icons_rename_reuses_thumbnail_details,
    idle_workers_exit_and_restart,
    invalid_archive_name_keeps_dialog,
    invalid_names_retain_original,
    invalid_new_file_names_corrected,
    keep_both_applies_to_all_mixed_paste,
    keep_both_numbered_archives,
    keep_both_then_undo,
    keep_both_then_undo_remaining,
    keyboard_context_menu_owns_keys,
    keyboard_only_copy_paste,
    keyboard_open_selects_first_child,
    keyboard_selection_in_single_click_mode,
    leftover_click_honors_modifiers,
    list_column_resize_tracks_pointer,
    list_icons_fill_duration_cached,
    list_preview_keeps_horizontal_scroll,
    list_transfer_reveal_selects_copy,
    list_typed_file_path,
    local_filtered_result_opens_once,
    long_rename_keeps_caret,
    markdown_equations_keep_prose,
    markdown_preview_renders_media,
    marquee_beside_content_full_pane,
    marquee_keeps_preview_closed,
    matching_rename_stays_searchable,
    media_details_properties_only,
    media_keys_step_timeline_and_volume,
    merge_combines_folder_contents,
    modal_dialog_blocks_window_shortcuts,
    model_preview_reports_limits,
    modifier_click_focuses_target,
    modifier_clicks_inert_space_select,
    multi_selection_drag_moves_every_entry,
    named_location_controls,
    narrow_window_keeps_last_column,
    navigation_survives_insertion,
    new_file_collision_dialog,
    new_folder_collision_dialog,
    new_folder_with_selection_groups,
    new_item_backspace_clears_name,
    new_item_clears_hiding_filter,
    new_item_first_free_number,
    new_items_in_empty_directory,
    one_click_only_selects,
    open_with_always_use_updates_default,
    open_with_broken_link_disabled,
    open_with_cancel_keeps_default,
    open_with_common_handlers_no_default,
    open_with_incompatible_types_other_apps,
    open_with_keeps_default,
    open_with_launch_failure_error,
    open_with_mixed_types_hidden_default,
    open_with_names_rows_tabs_out,
    open_with_search_filters_escape_clears,
    open_with_unknown_type_other_apps,
    opening_child_keeps_paste_under_pointer,
    outside_wheel_moves_pointed_column,
    overlapping_providers_source_priority,
    oversized_column_width_fits,
    pane_corner_marquee_keeps_sidebar,
    pane_menu_directory_actions,
    panel_wheel_routing,
    panel_wheel_routing_remaining,
    parent_pane_anchors_first_entry,
    paste_availability_follows_clipboard,
    paste_into_explicit_selection,
    paste_into_parent_current_directory,
    paste_targets_selected_directory,
    pdf_preview_copies_text,
    pdf_zoom_keeps_text_size,
    peek_click_reveals_column,
    permanent_delete_confirmation,
    permanent_delete_symlinked_parent,
    preselected_entry_hover,
    preview_blank_click_keeps_selection,
    preview_boundary_resizes_grabbed_side,
    preview_find_navigates_content,
    preview_follows_extended_selection,
    preview_follows_selection,
    preview_hides_on_folder,
    preview_hides_on_shift_range_folder,
    preview_mode_survives_unsupported,
    preview_source_text_copies,
    properties_media_metadata,
    properties_nested_hidden_sizes,
    properties_pins_and_unpins_folder,
    properties_unreadable_folder_explains,
    properties_zero_empty_folder,
    provider_background_clicked_folder,
    provider_continuous_events_progress,
    provider_disconnect_never_replays,
    provider_menu_replaces_context,
    provider_method_error_withdraws,
    provider_nested_menu_refresh,
    provider_partial_activation_whole_selection,
    provider_refresh_keeps_badges_menu,
    provider_updates_open_menu,
    raw_details_preview_and_properties,
    recent_absent_from_sidebar,
    recent_preference_adds_no_place,
    recent_uri_spellings_refused,
    recursive_file_double_click_launches_once,
    recursive_filtered_result_opens_once,
    refresh_keeps_cursor_and_viewport,
    refresh_keeps_filter_focus,
    refresh_reconciles_external_changes,
    release_outside_cancels_drag,
    rename_and_undo_restores,
    rename_commits_on_click_away,
    rename_commits_on_enter,
    rename_from_context_menu,
    rename_onto_existing_rejected,
    rename_shortcuts,
    rename_shortcuts_leave_location,
    renaming_open_folder_follows_it,
    rendered_document_word_selection,
    repeated_renames_select_stem,
    replace_restores_original_on_undo,
    replacing_duplicate_name_overwrites,
    return_restores_nested_scroll,
    reversing_sort_direction,
    right_click_keeps_multi_selection,
    right_click_selects_entry,
    row_padding_drag_view_policy,
    same_folder_copy_numbered_duplicate,
    sandboxed_office_shared_preview,
    scrolling_extends_marquee,
    second_selection_replaces_first,
    secondary_click_retargets_menu,
    select_all_selects_every_entry,
    selected_blank_space_starts_file_drag,
    selection_properties_aggregate_warnings,
    shift_after_escape_starts_on_focused,
    shift_arrow_extends_selection,
    shift_click_range,
    shift_click_range_keyboard_entry,
    shift_click_revisits_after_folder,
    shift_drag_moves_file,
    shortcut_reference_opens_and_closes,
    shortcut_round_trip_keeps_selection,
    show_thumbnails_switch_persists,
    sidebar_destinations_accept_three_files,
    sidebar_marquee_keeps_scrolled_list,
    sidebar_marquee_reaches_leading_pane,
    sidebar_navigation_range_anchor,
    sidebar_place_navigates,
    sidebar_selection_depends_on_activation,
    simple_click_opens_preview,
    single_click_clipped_parent_column,
    single_click_opens_directory,
    single_click_opens_directory_group,
    single_root_archive_extracts_verbatim,
    skipping_one_collision_pastes_rest,
    slow_click_away_does_not_rename,
    slow_click_rename_escape_stem,
    small_image_preview_native_size,
    sorting_by_size,
    space_completes_composition,
    space_enters_folder_without_preview,
    space_opens_and_closes_preview,
    space_previews_filtered_result,
    space_previews_filtered_result_pointer,
    spring_navigation_keeps_drag,
    strip_edge_scrolls_columns_in,
    submenu_arrows_return_to_parent,
    switching_keeps_directory_and_sort,
    switching_keeps_pane_filter,
    tab_drag_reorders_numbers,
    tabs_keep_locations_numbered,
    tenxer_d_without_trash_confirms,
    tenxer_escape_closes_peek,
    tenxer_escape_filter_preview_fill,
    tenxer_escape_visual_preview_search,
    tenxer_footer_filter_commits,
    tenxer_footer_find_moves_cursor,
    tenxer_footer_hit_path_follows_cursor,
    tenxer_footer_rename_refuses_conflicts,
    tenxer_footer_search_restores_filter,
    tenxer_go_chord_places_and_pins,
    tenxer_go_prompt_picks_folders,
    tenxer_history_prompts,
    tenxer_icons_edges_search_peek,
    tenxer_keeps_location_edit,
    tenxer_l_enters_preview,
    tenxer_numbered_action_confirms,
    tenxer_open_with_fill_launches_on_accept,
    tenxer_sidebar_header_round_trips,
    tenxer_sort_chords_keep_cursor,
    tenxer_space_toggles_cursor,
    tenxer_tab_chords,
    tenxer_visual_ranges,
    terminal_launchers_reaped,
    terminal_without_emulator_reports,
    thumbnail_workers_stepper_persists,
    tone_mark_escape_before_rename,
    trashing_open_folder_opens_next,
    two_clicks_open,
    two_slow_clicks_do_not_open,
    type_to_search_finds_nested,
    typed_dot_segments_resolve,
    typed_path_navigates,
    undo_compression_trashes_archive,
    undo_created_folder_trashes,
    undo_restores_completed_move,
    unreadable_location_reports_error,
    valid_name_commits_remaining,
    video_preview_badges_next_video,
    visible_created_item_preserves_scroll,
    visible_rename_preserves_scroll,
    wildcard_filter_patterns,
    window_accelerators_leave_rename,
    zero_selects_tenth_tab,
}

const REPOSITORY: &str = env!("CARGO_MANIFEST_DIR");
const BINARY: &str = env!("CARGO_BIN_EXE_strata");
/// `tree.DEFAULT_TIMEOUT`, as long as a Python `strata.wait` waits.
const DEFAULT_WAIT: Duration = Duration::from_millis(15_000);
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(90);
/// Covers the slowest command without a `timeout_ms`: `select` waits for its
/// entry twice and for the selection twice.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(90);
/// Covers the process's own wait plus the drains around it.
const WAIT_MARGIN: Duration = Duration::from_secs(30);
const EXIT_TIMEOUT: Duration = Duration::from_secs(10);
/// `TERMINATE_GRACE_SECONDS` in `tests/e2e/harness/process.py`.
const TERMINATE_GRACE: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(50);
const FIRST_DISPLAY: u32 = 90;
const DISPLAY_TRIES: u32 = 256;
const XVFB_START_TIMEOUT: Duration = Duration::from_secs(15);
const DEFAULT_LAYOUT: &str = "us";
const ALL_MODIFIERS: u16 = 0xff;
/// The error `strata --qa` replies with before it exits on a harness bug.
const CONTROLLER_PANIC: &str = "the QA controller panicked";
const RUNNER_COMMANDS: [&str; 12] = [
    "layout",
    "fs",
    "increment",
    "slice",
    "screenshot",
    "stem",
    "flag",
    "restart",
    "dbus_send",
    "log",
    "proc",
    "each",
];
/// Runner steps that only keep or switch state, which no `known_bug_step`
/// may name.
const KEEPING_STEPS: [&str; 8] = [
    "layout",
    "flag",
    "note",
    "slice",
    "increment",
    "stem",
    "screenshot",
    "restart",
];
/// What a run-time note stands for while steps are checked before the launch.
const NOTED_PLACEHOLDER: &str = "noted";
/// `ffmpeg` writing a generated media file, as a twin's `subprocess.run(..., timeout=30)`;
/// also ImageMagick's conversion and `dbus-send`.
const FFMPEG_TIMEOUT: Duration = Duration::from_secs(30);
/// How long the private session bus may take to listen, as `HeadlessDisplay`
/// waits `startup_timeout` for it.
const BUS_START_TIMEOUT: Duration = Duration::from_secs(30);
/// `SESSION_BUS_CONFIG` in `tests/e2e/harness/display.py`: a session bus with
/// no service directories, so nothing on it is D-Bus-activatable.
const SESSION_BUS_CONFIG: &str = r#"<!DOCTYPE busconfig PUBLIC
 "-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:path={socket}</listen>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
"#;
/// A step key the runner reads and strips: the modes the step runs in.
const STEP_MODES: &str = "modes";
/// A step key the runner reads and strips: the header variants the step runs
/// in, as a twin's `if target == "row-space"` branches.
const STEP_VARIANTS: &str = "variants";
/// A step key the runner reads and strips: a predicate the step runs only
/// while it holds, checked through the process's `holds` just before, as a
/// twin's `if node.has_state(...)` branches.
const STEP_WHEN: &str = "when";
/// Step keys the runner reads and strips: the step runs only if, or only
/// unless, an earlier `expect` kept as that name held, as a twin's
/// `if _focus_returned(...)` or `except TreeTimeout:` branches.
const STEP_IF: &str = "if";
const STEP_UNLESS: &str = "unless";
const MODES: [(&str, &str); 3] = [("columns", "Columns"), ("icons", "Icons"), ("list", "List")];
const DEFAULT_TREE: &str = "canonical";
/// What `state_parity_ignore` may name: `IGNORABLE_STATE` in `scripts/qa_parity.py`.
const IGNORABLE_STATE: [&str; 8] = [
    "view_mode",
    "directory",
    "panes",
    "focused",
    "focused_pane",
    "dialog",
    "panes.selected",
    "panes.focused",
];
/// Synthetic input the process sent, in the format `docs/e2e-testing.md` defines.
const INPUT_LOG: &str = "input.log";
/// The `app` snapshot after the last step, compared with the twin's final state.
const STATE_FILE: &str = "state.json";
/// The run's twin, errors, and failed steps, for `scripts/qa_parity.py`.
const RESULT_FILE: &str = "result.json";
/// Set to `1` by `scripts/qa_parity.py`: each run names its fixture directory
/// after its twin's node id, as the twin does, so typed paths and pane names
/// are the same on both sides.
const PARITY_FIXTURES: &str = "STRATA_PARITY_FIXTURES";
/// How long the final state may keep changing before it is taken anyway.
const STATE_SETTLE_TIMEOUT: Duration = Duration::from_secs(5);
const STATE_SETTLE_INTERVAL: Duration = Duration::from_millis(250);
/// Per-test paths the manifest leaves out of its static environment.
const XDG_DIRECTORIES: [(&str, &str); 5] = [
    ("HOME", "home"),
    ("XDG_CONFIG_HOME", "config"),
    ("XDG_DATA_HOME", "data"),
    ("XDG_CACHE_HOME", "cache"),
    ("XDG_STATE_HOME", "state"),
];
/// Variables the runner sets for every run, which a header's `env` may not
/// replace (nor the [`XDG_DIRECTORIES`] or [`QA_ENVIRONMENT`] ones).
const RUNNER_OWNED_ENVIRONMENT: [&str; 6] = [
    "DISPLAY",
    "DBUS_SESSION_BUS_ADDRESS",
    "XDG_RUNTIME_DIR",
    "STRATA_INPUT_LOG",
    PARITY_FIXTURES,
    "STRATA_STATE_DUMP",
];
/// No AT-SPI here: the process reads its own widget tree. A session bus
/// address that cannot connect keeps GLib from autolaunching one.
const QA_ENVIRONMENT: [(&str, &str); 2] = [("GTK_A11Y", "none"), ("NO_AT_BRIDGE", "1")];

// --------------------------------------------------------------- policy

fn require_container() {
    if std::env::var_os("STRATA_QA_CONTAINER").is_some_and(|value| value == "1") {
        return;
    }
    if std::env::var_os("STRATA_QA_HOST").is_some_and(|value| value == "1") {
        eprintln!(
            "UNSUPPORTED: STRATA_QA_HOST=1 runs QA scenarios on the host toolkit. \
             This run is not validation evidence; use ./scripts/qa.sh."
        );
        return;
    }
    panic!(
        "QA scenarios run only in the pinned container: use ./scripts/qa.sh \
         (STRATA_QA_CONTAINER=1 is set there). A host run needs the owner's explicit \
         consent and STRATA_QA_HOST=1, and is never validation evidence."
    );
}

// ------------------------------------------------------------- manifest

/// `tests/fixtures/harness.json`, shared with the Python harness.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    mtime: i64,
    trees: BTreeMap<String, Value>,
    preferences: OrderedMap,
    gtk_settings: String,
    portal_offer_marker: String,
    environment: BTreeMap<String, String>,
    layouts: BTreeMap<String, Layout>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Layout {
    layouts: String,
    group: u8,
    caps_lock: bool,
    variants: String,
}

/// Keeps document order, so `settings.toml` comes out as the Python harness writes it.
#[derive(Clone, Debug, Default)]
struct OrderedMap(Vec<(String, Value)>);

impl OrderedMap {
    /// Like Python's `dict.update`: replaced keys keep their place, new keys go last.
    fn update(&mut self, key: &str, value: Value) {
        match self.0.iter_mut().find(|(existing, _)| existing == key) {
            Some(entry) => entry.1 = value,
            None => self.0.push((key.to_owned(), value)),
        }
    }
}

impl<'de> Deserialize<'de> for OrderedMap {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = OrderedMap;
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("an object")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<OrderedMap, A::Error> {
                let mut entries = Vec::new();
                while let Some(entry) = map.next_entry::<String, Value>()? {
                    entries.push(entry);
                }
                Ok(OrderedMap(entries))
            }
        }
        deserializer.deserialize_map(Visitor)
    }
}

fn manifest() -> Manifest {
    let path = Path::new(REPOSITORY).join("tests/fixtures/harness.json");
    let text = fs::read_to_string(&path).expect("the harness manifest should be readable");
    serde_json::from_str(&text).expect("the harness manifest should parse")
}

fn render_toml(values: &OrderedMap) -> String {
    let mut output = String::new();
    for (key, value) in &values.0 {
        let _ = writeln!(output, "{key} = {}", render_value(value));
    }
    output
}

fn render_value(value: &Value) -> String {
    match value {
        Value::Bool(value) => value.to_string(),
        Value::Number(number) => number.to_string(),
        Value::Array(items) => format!(
            "[{}]",
            items
                .iter()
                .map(render_value)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::String(text) => format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\"")),
        Value::Null | Value::Object(_) => panic!("settings.toml cannot hold {value}"),
    }
}

fn populate(directory: &Path, tree: &Value) {
    let Value::Object(entries) = tree else {
        panic!("a fixture directory must be an object");
    };
    for (name, value) in entries {
        let path = directory.join(name);
        match value {
            Value::Object(_) => {
                fs::create_dir_all(&path).expect("the fixture directory should be created");
                fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
                    .expect("the fixture directory mode should be set");
                populate(&path, value);
            }
            Value::String(contents) => {
                fs::write(&path, contents).expect("the fixture file should be written");
                fs::set_permissions(&path, fs::Permissions::from_mode(0o644))
                    .expect("the fixture file mode should be set");
            }
            _ => panic!("fixture {name} must be a file's text or a directory"),
        }
    }
}

fn pin_times(path: &Path, mtime: SystemTime) {
    if path.is_dir() {
        let mut children: Vec<PathBuf> = fs::read_dir(path)
            .expect("the fixture directory should be listable")
            .map(|entry| entry.expect("the fixture entry should be readable").path())
            .collect();
        children.sort();
        for child in children {
            pin_times(&child, mtime);
        }
    }
    File::open(path)
        .and_then(|file| {
            file.set_times(fs::FileTimes::new().set_accessed(mtime).set_modified(mtime))
        })
        .expect("the fixture time should be pinned");
}

fn listing(root: &Path) -> String {
    fn walk(root: &Path, directory: &Path, lines: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(directory) else {
            return;
        };
        let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
        paths.sort();
        for path in paths {
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .display()
                .to_string();
            if path.is_symlink() {
                let target = fs::read_link(&path).unwrap_or_default();
                lines.push(format!("{relative} -> {}", target.display()));
            } else if path.is_dir() {
                lines.push(format!("{relative}/"));
                walk(root, &path, lines);
            } else {
                let size = fs::metadata(&path).map_or(0, |metadata| metadata.len());
                lines.push(format!("{relative} ({size} bytes)"));
            }
        }
    }
    let mut lines = Vec::new();
    walk(root, root, &mut lines);
    lines.join("\n")
}

// ------------------------------------------------------------- scenario

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    scenario: String,
    modes: Vec<String>,
    #[serde(default)]
    prefs: Map<String, Value>,
    #[serde(default = "default_layout")]
    layout: String,
    /// The pytest node each run ports; `$mode` and variant `$variables` are
    /// replaced per run, and a variant may name its own.
    twin: String,
    mutation: Option<String>,
    known_bug: Option<String>,
    /// The file line of the step a `known_bug` run must fail at; a failure
    /// anywhere else is an error.
    known_bug_step: Option<usize>,
    /// Runs of the file that differ in preferences, layout, or `$variables`.
    #[serde(default)]
    variants: Vec<Variant>,
    /// A manifest tree's name, or a tree written out here; see [`expand_tree`].
    tree: Option<Value>,
    /// Solid-colour PNGs written into the tree after its times are pinned, as
    /// a Python fixture saves a PIL image after `FixtureTree.create`.
    #[serde(default)]
    images: BTreeMap<String, Image>,
    /// Lines of the manifest's GTK `settings.ini` given another value, as a
    /// twin's `test_environment` override rewrites them.
    #[serde(default)]
    gtk_settings: BTreeMap<String, String>,
    /// Compared state keys final-state parity leaves out for this scenario,
    /// where the app itself ends nondeterministically; `scripts/qa_parity.py`
    /// reads them.
    #[serde(default)]
    state_parity_ignore: Vec<String>,
    /// Input events of one run whose pointer position input parity leaves
    /// out, where the app itself puts the target in either of two places
    /// under both harnesses; `scripts/qa_parity.py` reads them.
    #[serde(default)]
    input_parity_ignore: Vec<InputParityIgnore>,
    /// `fs` actions taken before the launch, as a twin's fixture writes files
    /// before `strata` starts: an executable in the tree, or desktop entries
    /// and actions under `$data` and `$config`.
    #[serde(default)]
    setup: Vec<Value>,
    /// `fs` actions taken once setup has begun, however the run ends, as a
    /// twin's `finally:` restores what it changed: `chmod` back to a readable mode.
    #[serde(default)]
    teardown: Vec<Value>,
    /// Variables the application's environment gets on top of the
    /// manifest's, `$variables` replaced, as a twin's `test_environment`
    /// override returns `{**variables(), "PATH": ...}`.
    #[serde(default)]
    env: BTreeMap<String, String>,
    /// A private session bus for the application and `dbus_send` steps, as
    /// the Python harness's `HeadlessDisplay` starts one; without it the
    /// application gets a bus address that cannot connect.
    #[serde(default)]
    session_bus: bool,
}

/// One run's pointer events whose position is not compared, each named by
/// its 1-based number among the run's key and pointer events and the event
/// expected there, so a step added before it cannot shift the exemption.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InputParityIgnore {
    run: String,
    events: Vec<IgnoredEvent>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct IgnoredEvent {
    position: usize,
    /// `button <n> down` or `button <n> up`.
    event: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Image {
    width: u32,
    height: u32,
    rgb: [u8; 3],
    /// For an fs `image` over a numbered range: this much wider per number,
    /// as `Image.new("RGB", (320 + index, 180), ...)` in a loop.
    #[serde(default)]
    width_step: u32,
    /// For an fs `image`: a cached thumbnail written right after each file,
    /// as a twin's loop saves the source and then its thumbnail.
    thumbnail: Option<Thumbnail>,
}

/// A freedesktop cached thumbnail: a solid-colour PNG named after the MD5 of
/// the source's URI in `cache`, with `Thumb::URI` and `Thumb::MTime` text
/// chunks, as the twins write one with `PngImagePlugin.PngInfo`; `mode` sets
/// the cache directory's mode, as `mkdir(parents=True, mode=0o700)`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Thumbnail {
    cache: String,
    width: u32,
    height: u32,
    rgb: [u8; 3],
    mode: Option<String>,
}

/// `Image.open(path).convert("RGB").getpixel(at) == rgb`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PixelAt {
    at: [u32; 2],
    rgb: [u8; 3],
}

/// `int(path.stat().st_mtime)`: seconds since the epoch, or a local time as
/// `int(datetime(2001, 9, 9, 1, 46, 40).timestamp())` reads it.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(untagged)]
enum Mtime {
    Epoch(i64),
    Local { local: [i32; 6] },
}

/// A gzipped tar of `[name, text]` members in order, each with this mode and
/// modification time, as `tarfile.open(path, "w:gz").addfile(info, ...)`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TarGz {
    mode: String,
    mtime: i64,
    members: Vec<(String, String)>,
}

/// The first desktop file `[Default Applications]` lists in a
/// `mimeapps.list` for the content type GIO gives a file, as
/// `ConfigParser()["Default Applications"][content_type].split(";")[0]`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DefaultApp {
    #[serde(rename = "for")]
    file: String,
    is: String,
}

/// The stub terminal launcher's `cwd<TAB>arguments` lines, as
/// `test_terminal` asserts on them: `count` lines, all the same, each
/// `--dir=` a folder whose resolved path is the resolved working directory
/// and lies inside `within`; or the first line's `--dir=` resolving to `first_dir`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Launches {
    count: Option<usize>,
    within: Option<String>,
    first_dir: Option<String>,
}

/// Every line of the files in a directory whose names end with `suffix`
/// that starts with `prefix` becomes `with`, as the trash scenario rewrites
/// each `.trashinfo` file's `DeletionDate=` line.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplaceLines {
    suffix: String,
    prefix: String,
    with: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Variant {
    id: String,
    #[serde(default)]
    modes: Option<Vec<String>>,
    #[serde(default)]
    prefs: Map<String, Value>,
    layout: Option<String>,
    #[serde(default)]
    vars: BTreeMap<String, String>,
    /// This variant's pytest node, when the header's pattern does not fit it.
    twin: Option<String>,
    /// This variant's fixture tree in place of the header's, as an indirect
    /// `fixture_tree` parametrization gives each case its own.
    tree: Option<Value>,
    /// `fs` actions taken after the header's `setup`, before the launch, as
    /// such a case's fixture writes its own file's bytes.
    #[serde(default)]
    setup: Vec<Value>,
}

fn default_layout() -> String {
    DEFAULT_LAYOUT.to_owned()
}

struct Scenario {
    header: Header,
    /// With their line numbers in the file.
    steps: Vec<(usize, Value)>,
}

/// Every scenario file, each under the folder named after its twin's test file.
fn scenario_files() -> Vec<PathBuf> {
    let directory = Path::new(REPOSITORY).join("tests/qa/scenarios");
    let mut files = Vec::new();
    for folder in fs::read_dir(&directory).expect("the scenario directory should be listable") {
        let folder = folder
            .expect("the scenario entry should be readable")
            .path();
        assert!(
            folder.is_dir(),
            "{}: scenarios live in a folder named after their twin's test file",
            folder.display()
        );
        for file in fs::read_dir(&folder).expect("the scenario folder should be listable") {
            let file = file.expect("the scenario entry should be readable").path();
            if file
                .extension()
                .is_some_and(|extension| extension == "jsonl")
            {
                files.push(file);
            }
        }
    }
    files.sort();
    files
}

fn scenario_path(name: &str) -> PathBuf {
    let file = format!("{}.jsonl", name.replace('_', "-"));
    scenario_files()
        .into_iter()
        .find(|path| path.file_name().is_some_and(|found| *found == *file))
        .unwrap_or_else(|| panic!("no scenario file {file} in tests/qa/scenarios"))
}

fn load_scenario(path: &Path) -> Result<Scenario, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut lines = text
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty());
    let (_, first) = lines.next().ok_or("the scenario is empty")?;
    let header: Header =
        serde_json::from_str(first).map_err(|error| format!("line 1, the header: {error}"))?;
    let expected = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    if header.scenario != expected {
        return Err(format!(
            "the header names {:?} but the file is {expected:?}",
            header.scenario
        ));
    }
    for mode in header.modes.iter().chain(
        header
            .variants
            .iter()
            .flat_map(|variant| variant.modes.iter().flatten()),
    ) {
        view_name(mode)?;
    }
    for tree in header.tree.iter().chain(
        header
            .variants
            .iter()
            .filter_map(|variant| variant.tree.as_ref()),
    ) {
        expand_tree(tree, &manifest())?;
    }
    for (index, step) in header
        .setup
        .iter()
        .chain(header.variants.iter().flat_map(|variant| &variant.setup))
        .enumerate()
    {
        let parsed: FsStep = serde_json::from_value(step.clone())
            .map_err(|error| format!("setup {}: invalid fs step: {error}", index + 1))?;
        if !parsed.fs.acts() || parsed.timeout_ms.is_some() || parsed.message.is_some() {
            return Err(format!(
                "setup {}: setup takes fs actions only, such as write or chmod",
                index + 1
            ));
        }
    }
    for (index, step) in header.teardown.iter().enumerate() {
        let parsed: FsStep = serde_json::from_value(step.clone())
            .map_err(|error| format!("teardown {}: invalid fs step: {error}", index + 1))?;
        if !parsed.fs.acts() || parsed.timeout_ms.is_some() || parsed.message.is_some() {
            return Err(format!(
                "teardown {}: teardown takes fs actions only, such as chmod",
                index + 1
            ));
        }
    }
    gtk_settings(&manifest().gtk_settings, &header.gtk_settings)?;
    for (index, key) in header.state_parity_ignore.iter().enumerate() {
        if !IGNORABLE_STATE.contains(&key.as_str())
            || header.state_parity_ignore[..index].contains(key)
        {
            return Err(format!(
                "state_parity_ignore takes distinct keys of {IGNORABLE_STATE:?}, not {key:?}"
            ));
        }
    }
    if !header.state_parity_ignore.is_empty() {
        if header.mutation.is_some() {
            return Err(
                "state_parity_ignore is for baseline parity; a scenario with a mutation keeps every check"
                    .to_owned(),
            );
        }
        // The top-level keys alone; ignoring all of them would compare nothing.
        if IGNORABLE_STATE
            .iter()
            .filter(|key| !key.contains('.'))
            .all(|key| {
                header
                    .state_parity_ignore
                    .iter()
                    .any(|ignored| ignored == key)
            })
        {
            return Err("state_parity_ignore must leave a compared key".to_owned());
        }
    }
    check_input_parity_ignore(&header)?;
    if let Some(name) = header.env.keys().find(|name| {
        RUNNER_OWNED_ENVIRONMENT.contains(&name.as_str())
            || XDG_DIRECTORIES.iter().any(|(variable, _)| variable == name)
            || QA_ENVIRONMENT.iter().any(|(variable, _)| variable == name)
    }) {
        return Err(format!(
            "env may not set {name}, which the runner owns for every run"
        ));
    }
    let mut steps = Vec::new();
    for (index, line) in lines {
        let step: Value =
            serde_json::from_str(line).map_err(|error| format!("line {}: {error}", index + 1))?;
        let Some(object) = step.as_object() else {
            return Err(format!("line {}: a step must be an object", index + 1));
        };
        let commands: Vec<&str> = protocol::COMMANDS
            .iter()
            .chain(RUNNER_COMMANDS.iter())
            .copied()
            .filter(|name| object.contains_key(*name))
            .collect();
        if commands.len() != 1 {
            return Err(format!(
                "line {}: a step needs exactly one command, found {commands:?}",
                index + 1
            ));
        }
        if let Some(modes) = object.get(STEP_MODES) {
            let modes: Vec<String> = serde_json::from_value(modes.clone())
                .map_err(|error| format!("line {}: modes: {error}", index + 1))?;
            for mode in &modes {
                view_name(mode).map_err(|error| format!("line {}: {error}", index + 1))?;
            }
        }
        if object.contains_key("dbus_send") && !header.session_bus {
            return Err(format!(
                "line {}: dbus_send needs the header's session_bus",
                index + 1
            ));
        }
        if let Some(variants) = object.get(STEP_VARIANTS) {
            let variants: Vec<String> = serde_json::from_value(variants.clone())
                .map_err(|error| format!("line {}: variants: {error}", index + 1))?;
            for variant in &variants {
                if !header.variants.iter().any(|known| known.id == *variant) {
                    return Err(format!("line {}: no variant {variant:?}", index + 1));
                }
            }
        }
        steps.push((index + 1, step));
    }
    match (&header.known_bug, header.known_bug_step) {
        (None, None) => {}
        (Some(_), Some(line)) => {
            let step = steps
                .iter()
                .find(|(number, _)| *number == line)
                .map(|(_, step)| step)
                .ok_or_else(|| format!("known_bug_step {line} is not a step line"))?;
            if let Some(kind) = KEEPING_STEPS
                .into_iter()
                .find(|kind| step.get(*kind).is_some())
            {
                return Err(format!(
                    "known_bug_step {line} is a {kind} step, which only keeps or switches state \
                     and cannot fail as the scenario's expected failure"
                ));
            }
            if [STEP_WHEN, STEP_IF, STEP_UNLESS]
                .iter()
                .any(|key| step.get(*key).is_some())
            {
                return Err(format!(
                    "known_bug_step {line} has a when, if, or unless, so it may not run at all"
                ));
            }
        }
        (Some(_), None) => {
            return Err("known_bug needs known_bug_step: the line expected to fail".to_owned());
        }
        (None, Some(_)) => return Err("known_bug_step needs known_bug".to_owned()),
    }
    Ok(Scenario { header, steps })
}

/// `input_parity_ignore` names distinct runs of the scenario, each with
/// distinct event numbers from 1, and only in a scenario without a mutation,
/// as `input_parity_ignore` in `scripts/qa_parity.py` checks.
fn check_input_parity_ignore(header: &Header) -> Result<(), String> {
    if header.input_parity_ignore.is_empty() {
        return Ok(());
    }
    if header.mutation.is_some() {
        return Err(
            "input_parity_ignore is for baseline parity; a scenario with a mutation keeps every check"
                .to_owned(),
        );
    }
    let labels: Vec<String> = runs(header).into_iter().map(|run| run.label).collect();
    for (index, entry) in header.input_parity_ignore.iter().enumerate() {
        if !labels.contains(&entry.run)
            || header.input_parity_ignore[..index]
                .iter()
                .any(|earlier| earlier.run == entry.run)
        {
            return Err(format!(
                "input_parity_ignore names distinct runs of {labels:?}, not {:?}",
                entry.run
            ));
        }
        let mut seen = Vec::new();
        for ignored in &entry.events {
            let named = ignored
                .event
                .strip_prefix("button ")
                .and_then(|rest| rest.split_once(' '))
                .is_some_and(|(button, direction)| {
                    button.parse::<u8>().is_ok() && matches!(direction, "down" | "up")
                });
            if ignored.position == 0 || seen.contains(&ignored.position) || !named {
                return Err(
                    "an input_parity_ignore event is {\"position\": N, \"event\": \"button 1 down\"} \
                     with a distinct position from 1"
                        .to_owned(),
                );
            }
            seen.push(ignored.position);
        }
        if seen.is_empty() {
            return Err("input_parity_ignore events name at least one event".to_owned());
        }
    }
    Ok(())
}

/// The view a mode draws, as `view_mode` predicates and `$view` spell it.
fn view_name(mode: &str) -> Result<&'static str, String> {
    MODES
        .iter()
        .find(|(name, _)| *name == mode)
        .map(|(_, view)| *view)
        .ok_or_else(|| format!("unknown mode {mode:?}"))
}

/// The steps one run executes: its mode's, with `$variables` replaced and the
/// runner-only `modes` key removed. Every step is checked here, before
/// anything launches.
fn run_steps(
    scenario: &Scenario,
    run: &Run,
    variables: &[(String, String)],
) -> Result<Vec<(usize, Value)>, String> {
    let mut steps = Vec::new();
    // Each note a step keeps, with a stand-in of its type for checking later steps.
    let mut notes: Vec<(String, Value)> = Vec::new();
    // The names earlier `expect` steps keep whether they held under.
    let mut flags: Vec<String> = Vec::new();
    for (line, step) in &scenario.steps {
        let mut step = substitute(step, variables);
        let Some(object) = step.as_object_mut() else {
            continue;
        };
        if let Some(modes) = object.remove(STEP_MODES)
            && !modes
                .as_array()
                .is_some_and(|modes| modes.iter().any(|mode| *mode == *run.mode))
        {
            continue;
        }
        if let Some(variants) = object.remove(STEP_VARIANTS)
            && !variants
                .as_array()
                .is_some_and(|variants| variants.iter().any(|id| *id == *run.variant.id))
        {
            continue;
        }
        let checked = substitute_notes(&step, &notes);
        check_step(&checked).map_err(|error| format!("line {line}: {error}"))?;
        // An each step's own item variables stand in for what each run reads.
        let checked = match checked.get("each") {
            Some(source) => {
                let source = source.as_str().unwrap_or_default();
                if !notes
                    .iter()
                    .any(|(note, value)| note == source && value.is_array())
                {
                    return Err(format!(
                        "line {line}: each needs an earlier note of a list, {source:?}"
                    ));
                }
                let each: EachStep = serde_json::from_value(checked.clone())
                    .map_err(|error| format!("line {line}: invalid each step: {error}"))?;
                let mut checked = checked.clone();
                checked["step"] = each.expand(&json!(NOTED_PLACEHOLDER))?;
                checked
            }
            None => checked,
        };
        for key in [STEP_IF, STEP_UNLESS] {
            if let Some(flag) = checked.get(key).and_then(Value::as_str)
                && !flags.iter().any(|kept| kept == flag)
            {
                return Err(format!(
                    "line {line}: {key} names {flag:?}, which no earlier expect or flag keeps"
                ));
            }
        }
        let keeps_flag = checked.get("expect").is_some()
            || checked.get("flag").is_some()
            || checked
                .get("log")
                .is_some_and(|log| log.get("contains").is_some());
        if keeps_flag && let Some(flag) = checked.get("as").and_then(Value::as_str) {
            flags.push(flag.to_owned());
        }
        if let Some(token) = unknown_variable(&checked) {
            return Err(format!(
                "line {line}: ${token} is neither a variable nor an earlier note"
            ));
        }
        if let Some((name, placeholder)) = kept_note(&checked, &notes)? {
            if variables.iter().any(|(variable, _)| *variable == name) {
                return Err(format!("line {line}: the note {name:?} hides a variable"));
            }
            notes.retain(|(existing, _)| *existing != name);
            notes.push((name, placeholder));
        }
        steps.push((*line, step));
    }
    if let Some(line) = scenario.header.known_bug_step
        && !steps.iter().any(|(number, _)| *number == line)
    {
        return Err(format!(
            "known_bug_step {line} does not run in the {} run",
            run.label
        ));
    }
    Ok(steps)
}

/// The name a `note` or `increment` step keeps its result under, with a
/// stand-in of the result's type: a list for list accessors, else text.
fn kept_note(step: &Value, notes: &[(String, Value)]) -> Result<Option<(String, Value)>, String> {
    let name = || step["as"].as_str().unwrap_or_default().to_owned();
    if step.get("note").is_some() {
        let Ok(protocol::Command::Note { value, .. }) = protocol::parse_command(&step.to_string())
        else {
            return Err("invalid note step".to_owned());
        };
        let Ok(protocol::Command::Note { pick, .. }) = protocol::parse_command(&step.to_string())
        else {
            return Err("invalid note step".to_owned());
        };
        let placeholder = if value == protocol::Accessor::ScrollTarget {
            json!(0.0)
        } else if value == protocol::Accessor::EditableFields {
            json!(0)
        } else if value == protocol::Accessor::Number {
            // A positive stand-in, so a noted size checks as a `resize` width.
            json!(1)
        } else if value.is_list() && pick.is_none() {
            json!([NOTED_PLACEHOLDER])
        } else {
            json!(NOTED_PLACEHOLDER)
        };
        return Ok(Some((name(), placeholder)));
    }
    if let Some(log) = step.get("log")
        && step.get("as").is_some()
        && log.get("contains").is_none()
    {
        // A count or a picked pid is a number; the pids a pattern finds a list.
        let placeholder = if log.get("matches").is_some() && log.get("pick").is_none() {
            json!([1])
        } else {
            json!(1)
        };
        return Ok(Some((name(), placeholder)));
    }
    if let Some(source) = step.get("stem") {
        let source = source.as_str().unwrap_or_default();
        if !notes
            .iter()
            .any(|(note, value)| note == source && value.is_string())
        {
            return Err(format!("stem needs an earlier note of a name, {source:?}"));
        }
        return Ok(Some((name(), json!(NOTED_PLACEHOLDER))));
    }
    if let Some(source) = step.get("increment") {
        let source = source.as_str().unwrap_or_default();
        if !notes
            .iter()
            .any(|(note, value)| note == source && value.is_string())
        {
            return Err(format!(
                "increment needs an earlier note of a name, {source:?}"
            ));
        }
        return Ok(Some((name(), json!(NOTED_PLACEHOLDER))));
    }
    if let Some(source) = step.get("slice") {
        let source = source.as_str().unwrap_or_default();
        if !notes
            .iter()
            .any(|(note, value)| note == source && value.is_array())
        {
            return Err(format!("slice needs an earlier note of a list, {source:?}"));
        }
        let placeholder = if step.get("pick").is_some() {
            json!(NOTED_PLACEHOLDER)
        } else {
            json!([NOTED_PLACEHOLDER])
        };
        return Ok(Some((name(), placeholder)));
    }
    Ok(None)
}

/// A `slice` step: `names[len(names) // 2:len(names) // 2 + middle]`, then
/// `names[pick]`, of a list a note kept, as a twin reuses one `rows` list.
fn slice(names: &[Value], middle: Option<u64>, pick: Option<i64>) -> Result<Value, String> {
    let mut names = names.to_vec();
    if let Some(count) = middle.map(|count| usize::try_from(count).unwrap_or(usize::MAX)) {
        let start = names.len() / 2;
        names = names[start..start.saturating_add(count).min(names.len())].to_vec();
    }
    let Some(pick) = pick else {
        return Ok(Value::Array(names));
    };
    let length = i64::try_from(names.len()).map_err(|error| error.to_string())?;
    let index = if pick < 0 { length + pick } else { pick };
    usize::try_from(index)
        .ok()
        .and_then(|index| names.get(index).cloned())
        .ok_or_else(|| format!("index {pick} is outside {names:?}"))
}

/// A `$name` left in any string once variables and notes were replaced;
/// `$$` is a literal `$`.
fn unknown_variable(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => {
            let mut rest = text.as_str();
            while let Some(start) = rest.find('$') {
                let after = &rest[start + 1..];
                if let Some(escaped) = after.strip_prefix('$') {
                    rest = escaped;
                    continue;
                }
                let token: String = after
                    .chars()
                    .take_while(|character| character.is_ascii_alphanumeric() || *character == '_')
                    .collect();
                if !token.is_empty() {
                    return Some(token);
                }
                rest = after;
            }
            None
        }
        Value::Array(items) => items.iter().find_map(unknown_variable),
        Value::Object(object) => object.values().find_map(unknown_variable),
        _ => None,
    }
}

/// Replaces notes kept while the run goes: a string that is exactly `$name`
/// becomes the note's value, list or not; elsewhere `$name` is its text.
fn substitute_notes(value: &Value, notes: &[(String, Value)]) -> Value {
    if notes.is_empty() {
        return value.clone();
    }
    match value {
        Value::String(text) => {
            if let Some((_, noted)) = text
                .strip_prefix('$')
                .and_then(|name| notes.iter().find(|(note, _)| note == name))
            {
                return noted.clone();
            }
            let texts: Vec<(String, String)> = notes
                .iter()
                .map(|(name, noted)| (name.clone(), python_text(noted)))
                .collect();
            Value::String(substitute_text(text, &texts))
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| substitute_notes(item, notes))
                .collect(),
        ),
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, item)| (key.clone(), substitute_notes(item, notes)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// A note as a twin's f-string shows it: text as is, a list as `['a', 'b']`.
fn python_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => "None".to_owned(),
        Value::Array(items) => format!(
            "[{}]",
            items
                .iter()
                .map(|item| match item {
                    Value::String(text) => format!("'{text}'"),
                    other => python_text(other),
                })
                .collect::<Vec<_>>()
                .join(", ")
        ),
        other => other.to_string(),
    }
}

/// `f"folder-{int(name.removeprefix('folder-')) + 1:03}"`: the text with its
/// trailing number one higher, zero-padded to the same width.
fn increment(text: &str) -> Result<String, String> {
    let digits = text.chars().rev().take_while(char::is_ascii_digit).count();
    let (prefix, number) = text.split_at(text.len() - digits);
    let value: u64 = number
        .parse()
        .map_err(|_| format!("{text:?} does not end in a number"))?;
    Ok(format!("{prefix}{:0width$}", value + 1, width = digits))
}

/// `Path(name).stem`: the name without its last extension.
fn stem(text: &str) -> Result<String, String> {
    Path::new(text)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .ok_or_else(|| format!("{text:?} has no stem"))
}

fn check_step(step: &Value) -> Result<(), String> {
    for key in ["increment", "stem"] {
        if step.get(key).is_some() {
            let object = step.as_object().ok_or("a step must be an object")?;
            let valid = object.len() == 2
                && step[key].is_string()
                && step["as"].as_str().is_some_and(|name| !name.is_empty());
            return if valid {
                Ok(())
            } else {
                Err(format!(
                    "{key} names a note and the name to keep the result as"
                ))
            };
        }
    }
    if step.get("holds").is_some() {
        return Err("holds is the runner's own check; a step uses when".to_owned());
    }
    if let Some(condition) = step.get("flag") {
        let valid = step.as_object().is_some_and(|object| object.len() == 2)
            && step["as"].as_str().is_some_and(|name| !name.is_empty());
        if !valid {
            return Err(
                "flag takes a predicate and the name to keep whether it held as".to_owned(),
            );
        }
        return protocol::parse_command(&json!({ "holds": condition }).to_string())
            .map(|_| ())
            .map_err(|error| format!("flag: {error}"));
    }
    if step.get("slice").is_some() {
        let object = step.as_object().ok_or("a step must be an object")?;
        let valid = step["slice"].is_string()
            && step["as"].as_str().is_some_and(|name| !name.is_empty())
            && object
                .keys()
                .all(|key| matches!(key.as_str(), "slice" | "as" | "middle" | "pick"))
            && object.get("middle").is_none_or(Value::is_u64)
            && object.get("pick").is_none_or(Value::is_i64)
            && (object.contains_key("middle") || object.contains_key("pick"));
        return if valid {
            Ok(())
        } else {
            Err(
                "slice names a list note, the name to keep the result as, and middle or pick"
                    .to_owned(),
            )
        };
    }
    if let Some(name) = step.get("screenshot") {
        let valid = step.as_object().is_some_and(|object| object.len() == 1)
            && name.as_str().is_some_and(|name| {
                !name.is_empty() && !name.contains('/') && name.ends_with(".png")
            });
        return if valid {
            Ok(())
        } else {
            Err(
                "screenshot names a .png file in the run's artifacts and takes nothing else"
                    .to_owned(),
            )
        };
    }
    // A flag that is never set would leave both an `if` and an `unless` step
    // out, so the expect that keeps it must always run.
    let keeps_flag = step.get("expect").is_some()
        || step
            .get("log")
            .is_some_and(|log| log.get("contains").is_some());
    if keeps_flag
        && step.get("as").is_some()
        && [STEP_WHEN, STEP_IF, STEP_UNLESS]
            .iter()
            .any(|key| step.get(*key).is_some())
    {
        return Err(
            "an expect or log that keeps a flag in as may not have a when, if, or unless"
                .to_owned(),
        );
    }
    for key in [STEP_IF, STEP_UNLESS] {
        if let Some(flag) = step.get(key) {
            if !flag.as_str().is_some_and(|flag| !flag.is_empty()) {
                return Err(format!("{key} names what an earlier expect kept"));
            }
            let mut step = step.clone();
            if let Some(object) = step.as_object_mut() {
                object.remove(key);
            }
            return check_step(&step);
        }
    }
    if step.get("expect").is_some()
        && step
            .get("as")
            .is_some_and(|name| !name.as_str().is_some_and(|name| !name.is_empty()))
    {
        return Err("an expect keeps whether it held under a name in as".to_owned());
    }
    if let Some(condition) = step.get(STEP_WHEN) {
        if ["note", "increment", "slice", "stem"]
            .iter()
            .any(|key| step.get(*key).is_some())
            || (step.get("log").is_some() && step.get("as").is_some())
        {
            return Err(
                "a note, increment, slice, or stem always runs; it takes no when".to_owned(),
            );
        }
        protocol::parse_command(&json!({ "holds": condition }).to_string())
            .map_err(|error| format!("when: {error}"))?;
        let mut step = step.clone();
        if let Some(object) = step.as_object_mut() {
            object.remove(STEP_WHEN);
        }
        return check_step(&step);
    }
    if let Some(layout) = step.get("layout") {
        let name = layout.as_str().ok_or("layout names a manifest layout")?;
        if step.as_object().is_some_and(|object| object.len() != 1) {
            return Err("layout takes no other fields".to_owned());
        }
        return if manifest().layouts.contains_key(name) {
            Ok(())
        } else {
            Err(format!("no layout {name:?} in the manifest"))
        };
    }
    if step.get("each").is_some() {
        let object = step.as_object().ok_or("a step must be an object")?;
        let each: EachStep = serde_json::from_value(Value::Object(object.clone()))
            .map_err(|error| format!("invalid each step: {error}"))?;
        if each.name.is_empty() || each.derive.keys().any(|key| key.is_empty()) {
            return Err("each keeps each item under a name in as".to_owned());
        }
        let inner = each.expand(&json!(NOTED_PLACEHOLDER))?;
        if RUNNER_COMMANDS
            .iter()
            .any(|command| inner.get(*command).is_some())
        {
            return Err("each repeats a step the process runs".to_owned());
        }
        return check_step(&inner);
    }
    if step.get("restart").is_some() {
        return if *step == json!({ "restart": true }) {
            Ok(())
        } else {
            Err("restart is {\"restart\": true} and takes nothing else".to_owned())
        };
    }
    if let Some(arguments) = step.get("dbus_send") {
        let valid = step.as_object().is_some_and(|object| object.len() == 1)
            && arguments.as_array().is_some_and(|arguments| {
                !arguments.is_empty() && arguments.iter().all(Value::is_string)
            });
        return if valid {
            Ok(())
        } else {
            Err("dbus_send takes the arguments after dbus-send --session".to_owned())
        };
    }
    if step.get("log").is_some() {
        let parsed: LogStep = serde_json::from_value(step.clone())
            .map_err(|error| format!("invalid log step: {error}"))?;
        return parsed.check();
    }
    if step.get("proc").is_some() {
        let parsed: ProcStep = serde_json::from_value(step.clone())
            .map_err(|error| format!("invalid proc step: {error}"))?;
        return parsed.check();
    }
    if step.get("fs").is_some() {
        let step: FsStep = serde_json::from_value(step.clone())
            .map_err(|error| format!("invalid fs step: {error}"))?;
        if step.fs.acts() && step.timeout_ms.is_some() {
            return Err(
                "an fs write or remember happens at once; it takes no timeout_ms".to_owned(),
            );
        }
        return step.fs.kind().map(|_| ());
    }
    protocol::parse_command(&step.to_string()).map(|_| ())
}

struct Run {
    mode: String,
    variant: Variant,
    label: String,
}

fn runs(header: &Header) -> Vec<Run> {
    let variants = if header.variants.is_empty() {
        vec![Variant::default()]
    } else {
        header.variants.clone()
    };
    let named = !header.variants.is_empty();
    variants
        .into_iter()
        .flat_map(|variant| {
            let modes = variant
                .modes
                .clone()
                .unwrap_or_else(|| header.modes.clone());
            modes.into_iter().map(move |mode| Run {
                label: if named {
                    format!("{mode}-{}", variant.id)
                } else {
                    mode.clone()
                },
                mode,
                variant: variant.clone(),
            })
        })
        .collect()
}

/// Where a run's files live, for the built-in `$variables`.
struct Places<'a> {
    fixture: &'a Path,
    /// The directory the run's XDG directories are made in.
    scratch_home: &'a Path,
}

impl Places<'_> {
    /// The directory the application sees as `variable`, from [`XDG_DIRECTORIES`].
    fn xdg(&self, variable: &str) -> PathBuf {
        let (_, directory) = XDG_DIRECTORIES
            .iter()
            .find(|(name, _)| *name == variable)
            .expect("every built-in names an XDG directory");
        self.scratch_home.join(directory)
    }
}

/// Stand-ins for checking steps before anything is created.
fn placeholders() -> Places<'static> {
    Places {
        fixture: Path::new("/tmp/strata-qa-fixture-placeholder"),
        scratch_home: Path::new("/tmp/strata-qa-placeholder/home"),
    }
}

/// A run's `$variables`, plus the built-ins: `$root`, the fixture directory's
/// name, which is also its pane's name; `$fixture`, its absolute path;
/// `$home`, `$data`, `$config`, and `$cache`, the run's HOME and XDG data,
/// config, and cache directories; `$environment`,
/// the directory holding them, a twin's `test_environment.root`; `$view`, the
/// run's view as `view_mode` names it; and `$mode`, its mode.
fn variables(run: &Run, places: &Places) -> Vec<(String, String)> {
    let mut variables: Vec<(String, String)> = run.variant.vars.clone().into_iter().collect();
    let root = places
        .fixture
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    variables.push(("root".to_owned(), root));
    variables.push(("fixture".to_owned(), places.fixture.display().to_string()));
    variables.push(("home".to_owned(), places.xdg("HOME").display().to_string()));
    variables.push((
        "data".to_owned(),
        places.xdg("XDG_DATA_HOME").display().to_string(),
    ));
    variables.push((
        "config".to_owned(),
        places.xdg("XDG_CONFIG_HOME").display().to_string(),
    ));
    variables.push((
        "cache".to_owned(),
        places.xdg("XDG_CACHE_HOME").display().to_string(),
    ));
    variables.push(("python".to_owned(), python()));
    variables.push((
        "fixture_quoted".to_owned(),
        quote_path(places.fixture.as_os_str().as_encoded_bytes()),
    ));
    variables.push((
        "view".to_owned(),
        view_name(&run.mode).unwrap_or_default().to_owned(),
    ));
    variables.push(("mode".to_owned(), run.mode.clone()));
    variables.push((
        "environment".to_owned(),
        places.scratch_home.display().to_string(),
    ));
    variables.sort_by_key(|(name, _)| std::cmp::Reverse(name.len()));
    variables
}

/// The interpreter a twin's `sys.executable` names in the pinned image: the
/// E2E virtual environment's `bin/python`, which `scripts/e2e-native.sh` runs
/// pytest with, or else `python3`.
fn python() -> String {
    std::env::var("STRATA_E2E_VENV").map_or_else(
        |_| "python3".to_owned(),
        |venv| format!("{}/bin/python", venv.trim_end_matches('/')),
    )
}

/// `urllib.parse.quote_from_bytes(path)`: every byte but ASCII letters,
/// digits, `_.-~`, and `/` as `%XX`, as `pathlib.Path.as_uri` escapes a path.
fn quote_path(bytes: &[u8]) -> String {
    let mut quoted = String::with_capacity(bytes.len());
    for byte in bytes {
        if byte.is_ascii_alphanumeric() || b"_.-~/".contains(byte) {
            quoted.push(char::from(*byte));
        } else {
            let _ = write!(quoted, "%{byte:02X}");
        }
    }
    quoted
}

/// The exact pytest node id a run ports.
fn twin(header: &Header, run: &Run) -> String {
    let pattern = run.variant.twin.as_deref().unwrap_or(&header.twin);
    substitute_text(pattern, &variables(run, &placeholders()))
}

/// Replaces `$name` in every string of a step; longer names first so `$next` never eats `$nextish`.
fn substitute(value: &Value, variables: &[(String, String)]) -> Value {
    match value {
        Value::String(text) => Value::String(substitute_text(text, variables)),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| substitute(item, variables))
                .collect(),
        ),
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, item)| (key.clone(), substitute(item, variables)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// `$name` is a whole token: `$view` leaves `$viewport` alone. `$$` stays
/// as it is for [`unescape`], so a later pass cannot read `$$E` as `$E`.
fn substitute_text(text: &str, variables: &[(String, String)]) -> String {
    let identifier = |character: char| character.is_ascii_alphanumeric() || character == '_';
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('$') {
        output.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        if let Some(escaped) = after.strip_prefix('$') {
            output.push_str("$$");
            rest = escaped;
            continue;
        }
        let length = after.find(|c: char| !identifier(c)).unwrap_or(after.len());
        let token = &after[..length];
        match variables.iter().find(|(name, _)| name == token) {
            Some((_, replacement)) if !token.is_empty() => output.push_str(replacement),
            _ => {
                output.push('$');
                output.push_str(token);
            }
        }
        rest = &after[length..];
    }
    output.push_str(rest);
    output
}

/// Every `$$` as the literal `$` it stands for, once nothing is left to replace.
fn unescape(value: &Value) -> Value {
    match value {
        Value::String(text) => Value::String(text.replace("$$", "$")),
        Value::Array(items) => Value::Array(items.iter().map(unescape).collect()),
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, item)| (key.clone(), unescape(item)))
                .collect(),
        ),
        other => other.clone(),
    }
}

// --------------------------------------------------------------- display

static DISPLAYS: Mutex<()> = Mutex::new(());
/// Held around every spawn, so only Xvfb ever inherits its `-displayfd` pipe.
static SPAWN: Mutex<()> = Mutex::new(());

fn spawn(command: &mut Command) -> Result<Child, String> {
    let _spawning = SPAWN.lock().map_err(|error| error.to_string())?;
    command.spawn().map_err(|error| error.to_string())
}

/// SIGTERM first, so Xvfb removes its lock file and socket.
fn terminate(child: &mut Child) {
    if matches!(child.try_wait(), Ok(Some(_))) {
        return;
    }
    let _ = rustix::process::kill_process(
        rustix::process::Pid::from_child(child),
        rustix::process::Signal::TERM,
    );
    let deadline = Instant::now() + EXIT_TIMEOUT;
    while Instant::now() < deadline {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

struct Display {
    name: String,
    server: Child,
    log: PathBuf,
    connection: RustConnection,
}

impl Display {
    /// An explicit number keeps clear of other displays; `-displayfd` confirms
    /// that this server, not a concurrently starting one, owns it.
    fn start(log: PathBuf) -> Result<Self, String> {
        let _guard = DISPLAYS.lock().map_err(|error| error.to_string())?;
        for number in FIRST_DISPLAY..FIRST_DISPLAY + DISPLAY_TRIES {
            if Path::new(&format!("/tmp/.X{number}-lock")).exists()
                || Path::new(&format!("/tmp/.X11-unix/X{number}")).exists()
            {
                continue;
            }
            if let Some(display) = Self::try_start(number, &log)? {
                return Ok(display);
            }
        }
        Err(format!(
            "no free X display in :{FIRST_DISPLAY}-:{}",
            FIRST_DISPLAY + DISPLAY_TRIES - 1
        ))
    }

    fn try_start(number: u32, log: &Path) -> Result<Option<Self>, String> {
        let io = |error: std::io::Error| error.to_string();
        let output = File::create(log).map_err(io)?;
        let errors = output.try_clone().map_err(io)?;
        let (reader, writer) = std::io::pipe().map_err(io)?;
        let mut command = Command::new("Xvfb");
        command
            .arg(format!(":{number}"))
            .args(["-displayfd", &writer.as_raw_fd().to_string()])
            .args(["-screen", "0", "1440x900x24", "-dpi", "96"])
            .args(["-nolisten", "tcp", "-noreset", "-ac"])
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .stdin(Stdio::null())
            .stdout(output)
            .stderr(errors);
        let mut server = {
            let _spawning = SPAWN.lock().map_err(|error| error.to_string())?;
            rustix::io::fcntl_setfd(&writer, rustix::io::FdFlags::empty())
                .map_err(|error| error.to_string())?;
            let spawned = command.spawn();
            rustix::io::fcntl_setfd(&writer, rustix::io::FdFlags::CLOEXEC)
                .map_err(|error| error.to_string())?;
            spawned.map_err(|error| format!("cannot start Xvfb: {error}"))?
        };
        let (sender, advertised) = mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let _ = BufReader::new(reader).read_line(&mut line);
            let _ = sender.send(line);
        });
        let deadline = Instant::now() + XVFB_START_TIMEOUT;
        let owned = loop {
            match advertised.recv_timeout(POLL_INTERVAL) {
                Ok(line) => break line == format!("{number}\n"),
                Err(RecvTimeoutError::Timeout)
                    if Instant::now() < deadline && matches!(server.try_wait(), Ok(None)) => {}
                Err(_) => break false,
            }
        };
        // Xvfb writes the number and the newline separately; the pipe stays
        // open until both arrived so the newline cannot hit EPIPE.
        drop(writer);
        let name = format!(":{number}");
        let connection = owned.then(|| x11rb::connect(Some(&name)).ok()).flatten();
        let Some((connection, _)) = connection else {
            terminate(&mut server);
            return Ok(None);
        };
        xkb::use_extension(&connection, 1, 0)
            .map_err(|error| error.to_string())?
            .reply()
            .map_err(|error| error.to_string())?;
        Ok(Some(Self {
            name,
            server,
            log: log.to_owned(),
            connection,
        }))
    }

    fn setxkbmap(&self, arguments: &[&str]) -> Result<String, String> {
        // The explicit display and the PATH-only environment keep this off the desktop.
        let output = spawn(
            Command::new("setxkbmap")
                .args(["-display", &self.name])
                .args(arguments)
                .env_clear()
                .env("PATH", std::env::var_os("PATH").unwrap_or_default())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped()),
        )
        .and_then(|child| child.wait_with_output().map_err(|error| error.to_string()))
        .map_err(|error| format!("cannot run setxkbmap: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "setxkbmap {} failed: {}",
                arguments.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// Locks `group`; of the `affect` modifiers, locks those in `locks` and unlocks the rest.
    fn lock(&self, group: u8, affect: u16, locks: u16) -> Result<(), String> {
        xkb::latch_lock_state(
            &self.connection,
            xkb::ID::USE_CORE_KBD.into(),
            xproto::ModMask::from(affect),
            xproto::ModMask::from(locks),
            true,
            xkb::Group::from(group),
            xproto::ModMask::from(0u16),
            false,
            0,
        )
        .map_err(|error| error.to_string())?;
        self.connection.sync().map_err(|error| error.to_string())
    }

    /// As `harness/keyboard_layout.py` does: load the layouts, then lock the group.
    fn switch_layout(&self, layout: &Layout) -> Result<(), String> {
        let mut arguments = vec!["-layout", layout.layouts.as_str()];
        if !layout.variants.is_empty() {
            arguments.extend(["-variant", layout.variants.as_str()]);
        }
        self.setxkbmap(&arguments)?;
        let caps = if layout.caps_lock {
            u16::from(xproto::ModMask::LOCK)
        } else {
            0
        };
        self.lock(layout.group, caps, caps)
    }

    /// setxkbmap keeps locks, so they go first; then US without variants, verified.
    fn restore_layout(&self) -> Result<(), String> {
        self.lock(0, ALL_MODIFIERS, 0)?;
        self.setxkbmap(&["-layout", DEFAULT_LAYOUT])?;
        let query = self.setxkbmap(&["-query"])?;
        let field = |name: &str| {
            query.lines().find_map(|line| {
                let (key, value) = line.split_once(':')?;
                (key.trim() == name).then(|| value.trim().to_owned())
            })
        };
        let state = xkb::get_state(&self.connection, xkb::ID::USE_CORE_KBD.into())
            .map_err(|error| error.to_string())?
            .reply()
            .map_err(|error| error.to_string())?;
        let clean = u8::from(state.group) == 0
            && u8::from(state.locked_group) == 0
            && u16::from(state.locked_mods) == 0;
        if field("layout").as_deref() != Some(DEFAULT_LAYOUT)
            || field("variant").is_some()
            || !clean
        {
            return Err(format!(
                "the keyboard was not restored: layout {:?}, variant {:?}, group {}, locked modifiers {:#x}",
                field("layout"),
                field("variant"),
                u8::from(state.group),
                u16::from(state.locked_mods)
            ));
        }
        Ok(())
    }

    fn screenshot(&self, destination: &Path) -> Result<(), String> {
        let status = spawn(
            Command::new("import")
                .args(["-display", &self.name, "-window", "root"])
                .arg(destination)
                .env_clear()
                .env("PATH", std::env::var_os("PATH").unwrap_or_default())
                .stdout(Stdio::null())
                .stderr(Stdio::null()),
        )
        .and_then(|mut child| child.wait().map_err(|error| error.to_string()))
        .map_err(|error| format!("cannot run import: {error}"))?;
        status
            .success()
            .then_some(())
            .ok_or_else(|| format!("import exited with {status}"))
    }
}

impl Drop for Display {
    fn drop(&mut self) {
        terminate(&mut self.server);
    }
}

/// The private session bus `HeadlessDisplay._start_session_bus` starts.
struct Bus {
    daemon: Child,
    address: String,
}

impl Bus {
    fn start(runtime: &Path) -> Result<Self, String> {
        let socket = runtime.join("bus");
        let config = runtime.join("session-bus.conf");
        fs::write(
            &config,
            SESSION_BUS_CONFIG.replace("{socket}", &socket.display().to_string()),
        )
        .map_err(|error| error.to_string())?;
        let log =
            File::create(runtime.join("dbus-daemon.log")).map_err(|error| error.to_string())?;
        let errors = log.try_clone().map_err(|error| error.to_string())?;
        let mut daemon = spawn(
            Command::new("dbus-daemon")
                .args(["--nofork", "--nopidfile"])
                .arg(format!("--config-file={}", config.display()))
                .env_clear()
                .env("PATH", std::env::var_os("PATH").unwrap_or_default())
                .stdin(Stdio::null())
                .stdout(log)
                .stderr(errors),
        )
        .map_err(|error| format!("cannot start dbus-daemon: {error}"))?;
        let deadline = Instant::now() + BUS_START_TIMEOUT;
        while !socket.exists() {
            if matches!(daemon.try_wait(), Ok(Some(_))) || Instant::now() >= deadline {
                terminate(&mut daemon);
                return Err(format!(
                    "the private session bus did not start: {}",
                    tail(&runtime.join("dbus-daemon.log"))
                ));
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        Ok(Self {
            daemon,
            address: format!("unix:path={}", socket.display()),
        })
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        terminate(&mut self.daemon);
    }
}

// ----------------------------------------------------------- application

/// What every launch of a run's application shares, so a `restart` starts
/// it again exactly as before.
struct Launch<'a> {
    manifest: &'a Manifest,
    home: &'a Path,
    fixture: &'a Path,
    display: &'a Display,
    runtime: &'a Path,
    log: PathBuf,
    input_log: &'a Path,
    /// The header's `env`, with `$variables` replaced.
    extra: Vec<(String, String)>,
    bus: Option<&'a str>,
}

impl Launch<'_> {
    fn start(&self) -> Result<Application, String> {
        Application::start(self)
    }

    /// The environment a twin's helper process gets: `process_environment()`,
    /// the test environment's variables, and the display's, as
    /// `test_file_manager_interface.call_file_manager` builds it.
    fn helper_environment(&self) -> Vec<(String, String)> {
        let mut variables = vec![("PATH".to_owned(), std::env::var("PATH").unwrap_or_default())];
        for (variable, directory) in XDG_DIRECTORIES {
            variables.push((
                variable.to_owned(),
                self.home.join(directory).display().to_string(),
            ));
        }
        variables.extend(
            self.manifest
                .environment
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
        variables.extend(self.extra.iter().cloned());
        variables.push(("DISPLAY".to_owned(), self.display.name.clone()));
        variables.push((
            "XDG_RUNTIME_DIR".to_owned(),
            self.runtime.display().to_string(),
        ));
        if let Some(bus) = self.bus {
            variables.push(("DBUS_SESSION_BUS_ADDRESS".to_owned(), bus.to_owned()));
        }
        variables
    }
}

struct Application {
    child: Child,
    stdin: Option<ChildStdin>,
    replies: Receiver<String>,
    log: PathBuf,
    /// False once a reply timed out or the process went away.
    responsive: bool,
}

impl Application {
    fn start(launch: &Launch) -> Result<Self, String> {
        let log = launch.log.clone();
        // A fresh log per launch, as `ManagedProcess.spawn` opens it "wb".
        let output = File::create(&log).map_err(|error| error.to_string())?;
        let mut command = Command::new(BINARY);
        // Its own group, as `ManagedProcess.spawn` starts a session, so a
        // restart can signal the application and whatever it started.
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        command
            .arg("--qa")
            .arg(launch.fixture)
            .current_dir(launch.fixture)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .envs(&launch.manifest.environment)
            .envs(QA_ENVIRONMENT)
            .env("DISPLAY", &launch.display.name)
            .env("XDG_RUNTIME_DIR", launch.runtime)
            .env("STRATA_INPUT_LOG", launch.input_log)
            .env(
                "DBUS_SESSION_BUS_ADDRESS",
                launch.bus.map_or_else(
                    || {
                        format!(
                            "unix:path={}",
                            launch.runtime.join("no-session-bus").display()
                        )
                    },
                    str::to_owned,
                ),
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(output);
        for (variable, directory) in XDG_DIRECTORIES {
            command.env(variable, launch.home.join(directory));
        }
        command.envs(launch.extra.iter().map(|(key, value)| (key, value)));
        let mut child =
            spawn(&mut command).map_err(|error| format!("cannot start {BINARY}: {error}"))?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let (sender, replies) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            stdin: child.stdin.take(),
            child,
            replies,
            log,
            responsive: true,
        })
    }

    fn receive(&mut self, timeout: Duration) -> Result<Value, String> {
        let received = self.replies.recv_timeout(timeout);
        self.responsive = received.is_ok();
        match received {
            Ok(line) => serde_json::from_str(&line)
                .map_err(|error| format!("unreadable reply {line:?}: {error}")),
            Err(RecvTimeoutError::Timeout) => {
                Err(format!("no reply within {} s", timeout.as_secs()))
            }
            Err(RecvTimeoutError::Disconnected) => {
                // Its output closes a moment before the exit can be reaped.
                let deadline = Instant::now() + EXIT_TIMEOUT;
                let status = loop {
                    match self.child.try_wait() {
                        Ok(Some(status)) => break status.to_string(),
                        Ok(None) if Instant::now() < deadline => std::thread::sleep(POLL_INTERVAL),
                        _ => break "still running".to_owned(),
                    }
                };
                Err(format!(
                    "strata exited ({status}); log tail:\n{}",
                    tail(&self.log)
                ))
            }
        }
    }

    fn send(&mut self, command: &Value, timeout: Duration) -> Result<Value, String> {
        let stdin = self.stdin.as_mut().ok_or("stdin is closed")?;
        writeln!(stdin, "{command}")
            .and_then(|()| stdin.flush())
            .map_err(|error| format!("cannot send {command}: {error}"))?;
        self.receive(timeout)
    }

    /// `harness.process.terminate`: SIGTERM to the group, a grace period,
    /// then SIGKILL and another, as `application.stop()` ends a twin's app.
    fn terminate_group(&mut self) {
        let group = rustix::process::Pid::from_child(&self.child);
        for signal in [rustix::process::Signal::TERM, rustix::process::Signal::KILL] {
            let _ = rustix::process::kill_process_group(group, signal);
            let deadline = Instant::now() + TERMINATE_GRACE;
            while Instant::now() < deadline {
                if matches!(self.child.try_wait(), Ok(Some(_))) {
                    return;
                }
                std::thread::sleep(POLL_INTERVAL);
            }
        }
        let _ = self.child.wait();
    }

    fn stop(&mut self) {
        drop(self.stdin.take());
        let deadline = Instant::now() + EXIT_TIMEOUT;
        while Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Application {
    fn drop(&mut self) {
        self.stop();
    }
}

fn tail(path: &Path) -> String {
    let text = fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(40)..].join("\n")
}

// ------------------------------------------------------------ filesystem

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FsStep {
    fs: FsCheck,
    timeout_ms: Option<u64>,
    /// What the step waits for, as the Python twin's `strata.wait` message says it.
    message: Option<String>,
}

/// One check or action about `path`, relative to the fixture root unless
/// absolute: wait for `exists`, `text`, `names`, `is` (`file`, `dir`, or
/// `symlink`), `empty`, the same bytes (`copy_of`) or size (`size_of`) as
/// another file; compare with an earlier listing (`unchanged`); or act at
/// once: `write` text (a last component like `{000..179}.txt` writes one file
/// per number, with `{}` in the text replaced by it), `remember` the tree's
/// listing under a name, `mkdir` (`Path.mkdir()`, with `parents` as
/// `mkdir(parents=True)`; a last component like `folder-{000..159}` makes one
/// directory per number), `chmod` to an octal mode, `populate` a tree there
/// and pin its times as `FixtureTree.populate` does, save a solid-colour PNG
/// (`image`), run `ffmpeg -v error -y <ffmpeg...> <path>`, both in place as
/// the twins write, or link a fresh directory on another file system there
/// (`volume`, the directory to make it in, named after `prefix`), removed
/// when the run ends; make a symbolic link to a target (`symlink`); copy a
/// program found on `PATH` with its mode and times (`program`, as
/// `shutil.copy2(shutil.which(...))`); wait until the text `has` some parts
/// and lacks others, until no name `names_without` a part, or until GIO says
/// whether the file can go to the Trash (`can_trash`).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FsCheck {
    path: String,
    exists: Option<bool>,
    text: Option<String>,
    names: Option<Vec<String>>,
    is: Option<String>,
    empty: Option<bool>,
    unchanged: Option<String>,
    copy_of: Option<String>,
    size_of: Option<String>,
    write: Option<String>,
    remember: Option<String>,
    mkdir: Option<bool>,
    parents: Option<bool>,
    chmod: Option<String>,
    populate: Option<Value>,
    image: Option<Image>,
    ffmpeg: Option<Vec<String>>,
    volume: Option<String>,
    prefix: Option<String>,
    /// Files written into a fresh volume before it is linked, as a twin's
    /// fixture fills its temporary directory before `symlink_to`.
    #[serde(default, rename = "with")]
    volume_files: BTreeMap<String, String>,
    symlink: Option<String>,
    program: Option<String>,
    has: Option<Has>,
    names_without: Option<String>,
    can_trash: Option<bool>,
    remove_volume: Option<bool>,
    remember_names: Option<String>,
    names_from: Option<String>,
    #[serde(default)]
    minus: Vec<String>,
    #[serde(default)]
    plus: Vec<String>,
    /// `path.unlink()`, or `shutil.rmtree(path)` for a directory.
    remove: Option<bool>,
    /// `path.rename(root / rename)`.
    rename: Option<String>,
    /// Keeps `path.read_bytes()` under a name, as a twin keeps `original`.
    keep_bytes: Option<String>,
    /// Waits until the file's bytes are those kept under a name.
    bytes_of: Option<String>,
    /// `len(path.read_text().splitlines())`: exactly, or `{"at_least":1}`; a
    /// missing file has no lines.
    lines: Option<LineCount>,
    /// A stored zip archive of `[name, contents]` entries in order, as
    /// `zipfile.ZipFile(path, "w")` writes them with `writestr`: text, or
    /// `{"image":{...}}` for a solid-colour PNG.
    zip: Option<Vec<(String, ZipEntry)>>,
    /// `path.open("wb").truncate(size)`: a sparse file this many bytes long.
    truncate: Option<u64>,
    /// `path.write_bytes((repository / name).read_bytes())`, in one write.
    from_repository: Option<String>,
    /// The path names a source file, and the step reads its cached
    /// thumbnail in this directory instead, for `exists`, `is`, `pixel`,
    /// `keep_mtime`, and `mtime_of`.
    cached_in: Option<String>,
    /// Waits until every entry of the directory but those ending in
    /// `except_suffix` has a cached thumbnail here, as `_folder_cached`.
    all_cached_in: Option<String>,
    except_suffix: Option<String>,
    /// Writes a cached thumbnail for the file at the path.
    thumbnail: Option<Thumbnail>,
    pixel: Option<PixelAt>,
    /// Keeps `path.stat().st_mtime_ns` under a name.
    keep_mtime: Option<String>,
    /// Waits until the modification time is the one kept under a name.
    mtime_of: Option<String>,
    /// `path.stat().st_mode & 0o111 == 0o111`.
    executable: Option<bool>,
    mtime: Option<Mtime>,
    /// The bytes in hexadecimal, written in one write.
    hex: Option<String>,
    /// The bytes in base64, written in one write.
    base64: Option<String>,
    /// `path.open("a").write(text)`.
    append: Option<String>,
    /// `harness.raw_photo.write_raw_photo(path)`: the synthetic DNG.
    raw_photo: Option<bool>,
    tar_gz: Option<TarGz>,
    /// The zip's member names are exactly these, in order, and each holds
    /// the bytes of the named fixture file, as `archive.namelist()` and
    /// `archive.read(name)` are compared.
    zip_members: Option<Vec<(String, String)>>,
    /// The same for a gzipped tar, as `getnames()` and `extractfile(...)`.
    tar_members: Option<Vec<(String, String)>>,
    /// `zipfile.is_zipfile(path)`.
    is_zip: Option<bool>,
    default_app: Option<DefaultApp>,
    /// The file's lines, each read with `Gio.File.new_for_commandline_arg`,
    /// are as many as these paths and include each, as the Open With twins
    /// compare what the recording launcher received.
    opened: Option<Vec<String>>,
    /// Values at dotted keys of the parsed TOML, as `saved["run"]["args"] == [...]`.
    toml: Option<Map<String, Value>>,
    /// The parsed TOML equals that of the bytes kept under a name.
    toml_like: Option<String>,
    /// Values at top-level keys of the parsed JSON, as `json.loads(...)["paths"] == [...]`.
    json: Option<Map<String, Value>>,
    /// The same, comparing each list as a set, as `set(record["paths"]) == {...}`.
    json_set: Option<Map<String, Value>>,
    launches: Option<Launches>,
    replace_lines: Option<ReplaceLines>,
    /// A one-page PDF showing a solid-colour image of this size at 72 dpi,
    /// as PIL saves an RGB image to a `.pdf` path.
    pdf_image: Option<Image>,
    /// `[magick or convert, *arguments, path]`, as the HEIC fixture converts.
    magick: Option<Vec<String>>,
    /// The path's `%XX` escapes stand for raw bytes, for names that are not
    /// UTF-8, as a twin opens `documents + b"/bad\xe8name.txt"`.
    #[serde(default)]
    percent_path: bool,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ZipEntry {
    Text(String),
    Image {
        image: Image,
    },
    /// `zipfile.ZipInfo(name, date_time=...)` with `external_attr = mode << 16`,
    /// written with `writestr(info, text)`.
    Member {
        text: String,
        date_time: [u16; 6],
        mode: String,
    },
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(untagged)]
enum LineCount {
    Exactly(usize),
    AtLeast { at_least: usize },
}

impl LineCount {
    fn matches(self, lines: usize) -> bool {
        match self {
            Self::Exactly(count) => lines == count,
            Self::AtLeast { at_least } => lines >= at_least,
        }
    }
}

/// Parts the text must contain and parts it must not; an unreadable file
/// contains nothing, as `_settings_has_key` treats an `OSError`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Has {
    #[serde(default)]
    contains: Vec<String>,
    #[serde(default)]
    lacks: Vec<String>,
}

enum FsKind<'a> {
    Exists(bool),
    Symlink(&'a str),
    Program(&'a str),
    Has(&'a Has),
    NamesWithout(&'a str),
    CanTrash(bool),
    RemoveVolume,
    Text(&'a str),
    Names(&'a [String]),
    Is(&'a str),
    Empty(bool),
    Unchanged(&'a str),
    CopyOf(&'a str),
    SizeOf(&'a str),
    Write(&'a str),
    Remember(&'a str),
    Mkdir { parents: bool },
    Chmod(u32),
    Populate(&'a Value),
    Image(&'a Image),
    Ffmpeg(&'a [String]),
    Volume(&'a str, &'a str, &'a BTreeMap<String, String>),
    RememberNames(&'a str),
    NamesFrom(&'a str),
    Remove,
    Rename(&'a str),
    Lines(LineCount),
    KeepBytes(&'a str),
    BytesOf(&'a str),
    Zip(&'a [(String, ZipEntry)]),
    Truncate(u64),
    FromRepository(&'a str),
    AllCached(&'a str, Option<&'a str>),
    Thumbnail(&'a Thumbnail),
    Pixel(&'a PixelAt),
    KeepMtime(&'a str),
    MtimeOf(&'a str),
    Executable(bool),
    Mtime(Mtime),
    Hex(&'a str),
    Base64(&'a str),
    Append(&'a str),
    RawPhoto,
    TarGz(&'a TarGz),
    ZipMembers(&'a [(String, String)]),
    TarMembers(&'a [(String, String)]),
    IsZip(bool),
    DefaultApp(&'a DefaultApp),
    Opened(&'a [String]),
    Toml(&'a Map<String, Value>),
    TomlLike(&'a str),
    Json(&'a Map<String, Value>),
    JsonSet(&'a Map<String, Value>),
    Launches(&'a Launches),
    ReplaceLines(&'a ReplaceLines),
    PdfImage(&'a Image),
    Magick(&'a [String]),
}

/// What fs steps keep while a run goes: `fixture.listing()` taken by
/// `remember`, by name, and the directories `volume` made, removed with it.
#[derive(Default)]
struct FsState {
    listings: BTreeMap<String, String>,
    /// File contents taken by `keep_bytes`, by name.
    bytes: BTreeMap<String, Vec<u8>>,
    /// `set(fixture.names(path))` taken by `remember_names`, by name.
    names: BTreeMap<String, Vec<String>>,
    volumes: Vec<tempfile::TempDir>,
    /// Where `remove` and `rename` may act: the fixture, the run's HOME and
    /// XDG directories, and its scratch directory.
    roots: Vec<PathBuf>,
    /// A read error a polled check met, which ends the wait at once, as the
    /// twin's `read_text()` or `read_bytes()` raises.
    fatal: Option<String>,
    /// Modification times taken by `keep_mtime`, in nanoseconds, by name.
    mtimes: BTreeMap<String, u128>,
}

/// Whether `path` lies inside one of `roots`: its parent directory with every
/// link resolved, then its own last component unresolved, so neither `..`
/// nor a linked directory reaches outside.
fn contained(path: &Path, roots: &[PathBuf]) -> Result<(), String> {
    let outside = || format!("{} lies outside the run's directories", path.display());
    let name = path
        .file_name()
        .filter(|name| *name != "." && *name != "..")
        .ok_or_else(outside)?;
    let parent = path
        .parent()
        .ok_or_else(outside)?
        .canonicalize()
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let resolved = parent.join(name);
    roots
        .iter()
        .filter_map(|root| root.canonicalize().ok())
        .any(|root| resolved.starts_with(&root) && resolved != root)
        .then_some(())
        .ok_or_else(outside)
}

impl FsCheck {
    fn kind(&self) -> Result<FsKind<'_>, String> {
        let mut kinds = Vec::new();
        if let Some(exists) = self.exists {
            kinds.push(FsKind::Exists(exists));
        }
        if let Some(text) = &self.text {
            kinds.push(FsKind::Text(text));
        }
        if let Some(names) = &self.names {
            kinds.push(FsKind::Names(names));
        }
        if let Some(kind) = &self.is {
            if !matches!(kind.as_str(), "file" | "dir" | "symlink") {
                return Err(format!("is takes file, dir, or symlink, not {kind:?}"));
            }
            kinds.push(FsKind::Is(kind));
        }
        if let Some(empty) = self.empty {
            kinds.push(FsKind::Empty(empty));
        }
        if let Some(name) = &self.unchanged {
            kinds.push(FsKind::Unchanged(name));
        }
        if let Some(other) = &self.copy_of {
            kinds.push(FsKind::CopyOf(other));
        }
        if let Some(other) = &self.size_of {
            kinds.push(FsKind::SizeOf(other));
        }
        if let Some(mode) = &self.chmod {
            let mode = u32::from_str_radix(mode, 8)
                .ok()
                .filter(|mode| *mode <= 0o7777)
                .ok_or_else(|| {
                    format!("chmod takes an octal mode such as \"755\", not {mode:?}")
                })?;
            kinds.push(FsKind::Chmod(mode));
        }
        if let Some(name) = &self.remember_names {
            kinds.push(FsKind::RememberNames(name));
        }
        if let Some(name) = &self.names_from {
            kinds.push(FsKind::NamesFrom(name));
        } else if !self.minus.is_empty() || !self.plus.is_empty() {
            return Err("minus and plus belong to names_from".to_owned());
        }
        if let Some(directory) = &self.volume {
            if !Path::new(directory).is_absolute() {
                return Err(format!(
                    "volume makes its directory in an absolute path, not {directory:?}"
                ));
            }
            if let Some(name) = self.volume_files.keys().find(|name| {
                let name = Path::new(name);
                name.is_absolute() || name.components().count() != 1
            }) {
                return Err(format!(
                    "with names files in the volume itself, not {name:?}"
                ));
            }
            kinds.push(FsKind::Volume(
                directory,
                self.prefix.as_deref().unwrap_or("strata-drop-volume-"),
                &self.volume_files,
            ));
        } else if self.prefix.is_some() || !self.volume_files.is_empty() {
            return Err("prefix and with belong to volume".to_owned());
        }
        if let Some(target) = &self.symlink {
            kinds.push(FsKind::Symlink(target));
        }
        if let Some(program) = &self.program {
            kinds.push(FsKind::Program(program));
        }
        if let Some(has) = &self.has {
            if has.contains.is_empty() && has.lacks.is_empty() {
                return Err("has needs contains or lacks".to_owned());
            }
            kinds.push(FsKind::Has(has));
        }
        if let Some(part) = &self.names_without {
            kinds.push(FsKind::NamesWithout(part));
        }
        if let Some(can_trash) = self.can_trash {
            kinds.push(FsKind::CanTrash(can_trash));
        }
        match self.remove {
            Some(true) => kinds.push(FsKind::Remove),
            Some(false) => return Err("remove is true or left out".to_owned()),
            None => {}
        }
        if let Some(name) = &self.rename {
            kinds.push(FsKind::Rename(name));
        }
        if let Some(count) = self.lines {
            kinds.push(FsKind::Lines(count));
        }
        if let Some(name) = &self.keep_bytes {
            kinds.push(FsKind::KeepBytes(name));
        }
        if let Some(name) = &self.bytes_of {
            kinds.push(FsKind::BytesOf(name));
        }
        match self.remove_volume {
            Some(true) => kinds.push(FsKind::RemoveVolume),
            Some(false) => return Err("remove_volume is true or left out".to_owned()),
            None => {}
        }
        if let Some(text) = &self.write {
            kinds.push(FsKind::Write(text));
        }
        if let Some(name) = &self.remember {
            kinds.push(FsKind::Remember(name));
        }
        match (self.mkdir, self.parents) {
            (Some(true), parents) => kinds.push(FsKind::Mkdir {
                parents: parents == Some(true),
            }),
            (Some(false), _) => return Err("mkdir is true or left out".to_owned()),
            (None, Some(_)) => return Err("parents belongs to mkdir".to_owned()),
            (None, None) => {}
        }
        if let Some(tree) = &self.populate {
            expand_directory(tree)?;
            kinds.push(FsKind::Populate(tree));
        }
        if let Some(image) = &self.image {
            kinds.push(FsKind::Image(image));
        }
        if let Some(arguments) = &self.ffmpeg {
            kinds.push(FsKind::Ffmpeg(arguments));
        }
        if let Some(entries) = &self.zip {
            kinds.push(FsKind::Zip(entries));
        }
        if let Some(size) = self.truncate {
            kinds.push(FsKind::Truncate(size));
        }
        if let Some(name) = &self.from_repository {
            if Path::new(name).is_absolute() || name.split('/').any(|part| part == "..") {
                return Err(format!(
                    "from_repository names a file inside the repository, not {name:?}"
                ));
            }
            kinds.push(FsKind::FromRepository(name));
        }
        if let Some(cache) = &self.all_cached_in {
            kinds.push(FsKind::AllCached(cache, self.except_suffix.as_deref()));
        } else if self.except_suffix.is_some() {
            return Err("except_suffix belongs to all_cached_in".to_owned());
        }
        if let Some(thumbnail) = &self.thumbnail {
            kinds.push(FsKind::Thumbnail(thumbnail));
        }
        if let Some(pixel) = &self.pixel {
            kinds.push(FsKind::Pixel(pixel));
        }
        if let Some(name) = &self.keep_mtime {
            kinds.push(FsKind::KeepMtime(name));
        }
        if let Some(name) = &self.mtime_of {
            kinds.push(FsKind::MtimeOf(name));
        }
        if let Some(executable) = self.executable {
            kinds.push(FsKind::Executable(executable));
        }
        if let Some(mtime) = self.mtime {
            kinds.push(FsKind::Mtime(mtime));
        }
        if let Some(text) = &self.hex {
            hex::decode(text).map_err(|error| format!("hex: {error}"))?;
            kinds.push(FsKind::Hex(text));
        }
        if let Some(text) = &self.base64 {
            kinds.push(FsKind::Base64(text));
        }
        if let Some(text) = &self.append {
            kinds.push(FsKind::Append(text));
        }
        match self.raw_photo {
            Some(true) => kinds.push(FsKind::RawPhoto),
            Some(false) => return Err("raw_photo is true or left out".to_owned()),
            None => {}
        }
        if let Some(archive) = &self.tar_gz {
            u32::from_str_radix(&archive.mode, 8)
                .map_err(|_| format!("tar_gz mode is octal, not {:?}", archive.mode))?;
            kinds.push(FsKind::TarGz(archive));
        }
        if let Some(members) = &self.zip_members {
            kinds.push(FsKind::ZipMembers(members));
        }
        if let Some(members) = &self.tar_members {
            kinds.push(FsKind::TarMembers(members));
        }
        if let Some(is_zip) = self.is_zip {
            kinds.push(FsKind::IsZip(is_zip));
        }
        if let Some(default) = &self.default_app {
            kinds.push(FsKind::DefaultApp(default));
        }
        if let Some(paths) = &self.opened {
            kinds.push(FsKind::Opened(paths));
        }
        if let Some(values) = &self.toml {
            kinds.push(FsKind::Toml(values));
        }
        if let Some(name) = &self.toml_like {
            kinds.push(FsKind::TomlLike(name));
        }
        if let Some(values) = &self.json {
            kinds.push(FsKind::Json(values));
        }
        if let Some(values) = &self.json_set {
            kinds.push(FsKind::JsonSet(values));
        }
        if let Some(launches) = &self.launches {
            let full = launches.count.is_some() && launches.within.is_some();
            let first = launches.first_dir.is_some()
                && launches.count.is_none()
                && launches.within.is_none();
            if full == first {
                return Err("launches takes count with within, or first_dir".to_owned());
            }
            kinds.push(FsKind::Launches(launches));
        }
        if let Some(replace) = &self.replace_lines {
            kinds.push(FsKind::ReplaceLines(replace));
        }
        if let Some(image) = &self.pdf_image {
            kinds.push(FsKind::PdfImage(image));
        }
        if let Some(arguments) = &self.magick {
            kinds.push(FsKind::Magick(arguments));
        }
        if self.cached_in.is_some()
            && !matches!(
                kinds.as_slice(),
                [FsKind::Exists(_)
                    | FsKind::Is(_)
                    | FsKind::Pixel(_)
                    | FsKind::KeepMtime(_)
                    | FsKind::MtimeOf(_)]
            )
        {
            return Err(
                "cached_in reads a thumbnail for exists, is, pixel, keep_mtime, or mtime_of"
                    .to_owned(),
            );
        }
        match kinds.len() {
            1 => Ok(kinds.remove(0)),
            _ => Err(
                "fs needs a path and exactly one of exists, text, names, is, empty, \
                 unchanged, copy_of, size_of, names_from, has, names_without, can_trash, \
                 lines, bytes_of, write, remember, remember_names, keep_bytes, mkdir, chmod, \
                 populate, image, ffmpeg, zip, truncate, from_repository, volume, \
                 remove_volume, symlink, program, remove, rename, all_cached_in, thumbnail, \
                 pixel, keep_mtime, mtime_of, executable, mtime, hex, base64, append, \
                 raw_photo, tar_gz, zip_members, tar_members, is_zip, default_app, opened, \
                 toml, toml_like, json, json_set, launches, replace_lines, pdf_image, or magick"
                    .to_owned(),
            ),
        }
    }

    fn acts(&self) -> bool {
        matches!(
            self.kind(),
            Ok(FsKind::Write(_)
                | FsKind::Remember(_)
                | FsKind::Mkdir { .. }
                | FsKind::Chmod(_)
                | FsKind::Populate(_)
                | FsKind::Image(_)
                | FsKind::Ffmpeg(_)
                | FsKind::Volume(..)
                | FsKind::Symlink(_)
                | FsKind::Program(_)
                | FsKind::RemoveVolume
                | FsKind::RememberNames(_)
                | FsKind::Remove
                | FsKind::Rename(_)
                | FsKind::KeepBytes(_)
                | FsKind::Zip(_)
                | FsKind::Truncate(_)
                | FsKind::FromRepository(_)
                | FsKind::Thumbnail(_)
                | FsKind::KeepMtime(_)
                | FsKind::Hex(_)
                | FsKind::Base64(_)
                | FsKind::Append(_)
                | FsKind::RawPhoto
                | FsKind::TarGz(_)
                | FsKind::ReplaceLines(_)
                | FsKind::PdfImage(_)
                | FsKind::Magick(_))
        )
    }

    fn observe(
        &self,
        root: &Path,
        fixture_time: SystemTime,
        state: &mut FsState,
    ) -> Result<(), String> {
        let mut path = if self.percent_path {
            root.join(
                <std::ffi::OsString as std::os::unix::ffi::OsStringExt>::from_vec(percent_decoded(
                    &self.path,
                )?),
            )
        } else {
            root.join(&self.path)
        };
        if let Some(cache) = &self.cached_in {
            path = cached_thumbnail(Path::new(cache), &path);
        }
        // The batch-six actions and record checks stay inside the run's
        // directories, as remove and rename do.
        let roots = state.roots.clone();
        let guarded = matches!(
            self.kind()?,
            FsKind::Thumbnail(_)
                | FsKind::Hex(_)
                | FsKind::Base64(_)
                | FsKind::Append(_)
                | FsKind::RawPhoto
                | FsKind::TarGz(_)
                | FsKind::PdfImage(_)
                | FsKind::Magick(_)
                | FsKind::DefaultApp(_)
                | FsKind::Opened(_)
                | FsKind::Toml(_)
                | FsKind::TomlLike(_)
                | FsKind::Json(_)
                | FsKind::JsonSet(_)
                | FsKind::Launches(_)
                | FsKind::ReplaceLines(_)
        ) || (self.percent_path && self.acts());
        if guarded {
            contained(&path, &roots)?;
        }
        let listings = &mut state.listings;
        let other = |name: &str| root.join(name);
        match self.kind()? {
            FsKind::Exists(exists) => {
                let actual = path.symlink_metadata().is_ok();
                (actual == exists)
                    .then_some(())
                    .ok_or(format!("exists is {actual}"))
            }
            FsKind::Text(text) => match fs::read_to_string(&path) {
                Ok(actual) if actual == text => Ok(()),
                Ok(actual) => Err(format!("text is {actual:?}")),
                Err(error) => Err(error.to_string()),
            },
            FsKind::Names(names) => {
                let actual = sorted_names(&path)?;
                (actual == names)
                    .then_some(())
                    .ok_or(format!("names are {actual:?}"))
            }
            FsKind::RememberNames(name) => {
                state.names.insert(name.to_owned(), sorted_names(&path)?);
                Ok(())
            }
            // `set(fixture.names()) == original - {...} | {...}`.
            FsKind::NamesFrom(name) => {
                let mut expected: Vec<String> = state
                    .names
                    .get(name)
                    .ok_or_else(|| format!("no names were remembered as {name:?}"))?
                    .iter()
                    .filter(|entry| !self.minus.contains(entry))
                    .chain(&self.plus)
                    .cloned()
                    .collect();
                expected.sort();
                expected.dedup();
                let actual = sorted_names(&path)?;
                (actual == expected)
                    .then_some(())
                    .ok_or(format!("names are {actual:?}, not {expected:?}"))
            }
            // `Path.is_file()` and `is_dir()` follow links; `is_symlink()` does not.
            FsKind::Is(kind) => {
                let holds = match kind {
                    "file" => path.is_file(),
                    "dir" => path.is_dir(),
                    _ => path.is_symlink(),
                };
                holds.then_some(()).ok_or(format!("is not a {kind}"))
            }
            // `not (path.exists() and any(path.iterdir()))`: a missing directory is empty.
            FsKind::Empty(empty) => {
                let actual = match fs::read_dir(&path) {
                    Ok(entries) => entries.count() == 0,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
                    Err(error) => return Err(error.to_string()),
                };
                (actual == empty)
                    .then_some(())
                    .ok_or(format!("empty is {actual}"))
            }
            FsKind::Unchanged(name) => {
                let before = listings
                    .get(name)
                    .ok_or_else(|| format!("no listing was remembered as {name:?}"))?;
                let now = listing(&path);
                (now == *before)
                    .then_some(())
                    .ok_or(format!("the listing changed to:\n{now}"))
            }
            FsKind::CopyOf(source) => {
                let read = |path: &Path| {
                    fs::read(path).map_err(|error| format!("{}: {error}", path.display()))
                };
                let (copy, source) = (read(&path)?, read(&other(source))?);
                (path.is_file() && copy == source)
                    .then_some(())
                    .ok_or(format!("the bytes differ: {copy:?}"))
            }
            FsKind::SizeOf(source) => {
                let size = |path: &Path| {
                    fs::metadata(path)
                        .map(|metadata| metadata.len())
                        .map_err(|error| format!("{}: {error}", path.display()))
                };
                let (copy, source) = (size(&path)?, size(&other(source))?);
                (copy == source)
                    .then_some(())
                    .ok_or(format!("the size is {copy}, not {source}"))
            }
            // `path.write_text(...)` per name, as a twin's loop writes numbered files.
            FsKind::Write(text) => {
                let parent = path.parent().unwrap_or(root);
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                for (name, number) in numbered(&name)? {
                    // A plain name is written as given, so one that is not UTF-8 survives.
                    let (target, text) = match number {
                        Some(number) => {
                            (parent.join(&name), text.replace("{}", &number.to_string()))
                        }
                        None => (path.clone(), (*text).to_owned()),
                    };
                    fs::write(target, text).map_err(|error| format!("{name}: {error}"))?;
                }
                Ok(())
            }
            FsKind::Chmod(mode) => fs::set_permissions(&path, fs::Permissions::from_mode(mode))
                .map_err(|error| error.to_string()),
            // `tempfile.TemporaryDirectory(prefix="strata-drop-volume-", dir=...)`,
            // which must lie on another device than the fixture, then linked in.
            FsKind::Volume(directory, prefix, files) => {
                let volume = tempfile::Builder::new()
                    .prefix(prefix)
                    .tempdir_in(directory)
                    .map_err(|error| format!("cannot make a directory in {directory}: {error}"))?;
                let device = |path: &Path| {
                    fs::metadata(path)
                        .map(|metadata| std::os::unix::fs::MetadataExt::dev(&metadata))
                        .map_err(|error| format!("{}: {error}", path.display()))
                };
                if device(volume.path())? == device(root)? {
                    return Err(format!("{directory} is on the fixture's own device"));
                }
                for (name, text) in files {
                    fs::write(volume.path().join(name), text).map_err(|error| error.to_string())?;
                }
                std::os::unix::fs::symlink(volume.path(), &path)
                    .map_err(|error| error.to_string())?;
                state.volumes.push(volume);
                Ok(())
            }
            FsKind::Remember(name) => {
                listings.insert(name.to_owned(), listing(&path));
                Ok(())
            }
            FsKind::Symlink(target) => {
                std::os::unix::fs::symlink(target, &path).map_err(|error| error.to_string())
            }
            FsKind::Program(name) => copy_program(name, &path),
            // The twin's `TemporaryDirectory` ends with its fixture, before the
            // state dump, and leaves the link dangling.
            FsKind::RemoveVolume => {
                let target = fs::read_link(&path).map_err(|error| error.to_string())?;
                let index = state
                    .volumes
                    .iter()
                    .position(|volume| volume.path() == target)
                    .ok_or_else(|| {
                        format!("{} links to no volume this run made", path.display())
                    })?;
                state
                    .volumes
                    .remove(index)
                    .close()
                    .map_err(|error| error.to_string())
            }
            FsKind::Has(has) => {
                let text = fs::read_to_string(&path).ok();
                let contains = |part: &String| {
                    text.as_deref()
                        .is_some_and(|text| text.contains(part.as_str()))
                };
                let missing: Vec<&String> =
                    has.contains.iter().filter(|part| !contains(part)).collect();
                let present: Vec<&String> =
                    has.lacks.iter().filter(|part| contains(part)).collect();
                if missing.is_empty() && present.is_empty() {
                    Ok(())
                } else {
                    Err(format!("lacks {missing:?} and has {present:?}"))
                }
            }
            FsKind::NamesWithout(part) => {
                let names = sorted_names(&path)?;
                let found: Vec<&String> = names.iter().filter(|name| name.contains(part)).collect();
                found
                    .is_empty()
                    .then_some(())
                    .ok_or(format!("names {found:?} contain {part:?}"))
            }
            // `Gio.File.query_info(FILE_ATTRIBUTE_ACCESS_CAN_TRASH)`, as the twin's
            // precondition asks GIO itself.
            FsKind::CanTrash(expected) => {
                use gtk::gio::prelude::FileExt as _;
                let actual = gtk::gio::File::for_path(&path)
                    .query_info(
                        gtk::gio::FILE_ATTRIBUTE_ACCESS_CAN_TRASH,
                        gtk::gio::FileQueryInfoFlags::NONE,
                        gtk::gio::Cancellable::NONE,
                    )
                    .map_err(|error| error.to_string())?
                    .boolean(gtk::gio::FILE_ATTRIBUTE_ACCESS_CAN_TRASH);
                (actual == expected)
                    .then_some(())
                    .ok_or(format!("can_trash is {actual}"))
            }
            FsKind::Mkdir { parents } => {
                let parent = path.parent().unwrap_or(root);
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                for name in expand_name(&name)? {
                    let made = if parents {
                        fs::create_dir_all(parent.join(&name))
                    } else {
                        fs::create_dir(parent.join(&name))
                    };
                    made.map_err(|error| format!("{name}: {error}"))?;
                }
                Ok(())
            }
            FsKind::Populate(tree) => {
                fs::create_dir_all(&path).map_err(|error| error.to_string())?;
                populate(&path, &expand_directory(tree)?);
                pin_times(&path, fixture_time);
                Ok(())
            }
            // PIL's buffered save writes a file this small in one write; written in
            // pieces, the listing would see it grow and replace its row.
            FsKind::Image(image) => {
                let parent = path.parent().unwrap_or(root);
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                for (name, number) in numbered(&name)? {
                    let target = parent.join(&name);
                    let wider = image.width_step * number.unwrap_or_default();
                    let bytes = png(&Image {
                        width: image.width + wider,
                        height: image.height,
                        rgb: image.rgb,
                        width_step: 0,
                        thumbnail: None,
                    })?;
                    fs::write(&target, bytes).map_err(|error| format!("{name}: {error}"))?;
                    if let Some(thumbnail) = &image.thumbnail {
                        contained(&target, &roots)?;
                        write_thumbnail(thumbnail, &target, &roots)?;
                    }
                }
                Ok(())
            }
            FsKind::AllCached(cache, except) => {
                let mut missing = Vec::new();
                for entry in fs::read_dir(&path).map_err(|error| error.to_string())? {
                    let source = entry.map_err(|error| error.to_string())?.path();
                    let name = source
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    if except.is_some_and(|suffix| name.ends_with(suffix)) {
                        continue;
                    }
                    if !cached_thumbnail(Path::new(cache), &source).is_file() {
                        missing.push(name);
                    }
                }
                missing.sort();
                missing
                    .is_empty()
                    .then_some(())
                    .ok_or(format!("no cached thumbnail for {missing:?}"))
            }
            FsKind::Thumbnail(thumbnail) => write_thumbnail(thumbnail, &path, &roots),
            FsKind::Pixel(pixel) => {
                let image = image::open(&path)
                    .map_err(|error| format!("{}: {error}", path.display()))?
                    .to_rgb8();
                let [x, y] = pixel.at;
                let found = image
                    .get_pixel_checked(x, y)
                    .map(|colour| colour.0)
                    .ok_or_else(|| format!("{x},{y} lies outside the image"))?;
                (found == pixel.rgb)
                    .then_some(())
                    .ok_or(format!("the pixel is {found:?}"))
            }
            FsKind::KeepMtime(name) => {
                let mtime = modified_nanos(&path)?;
                state.mtimes.insert(name.to_owned(), mtime);
                Ok(())
            }
            FsKind::MtimeOf(name) => {
                let kept = *state
                    .mtimes
                    .get(name)
                    .ok_or_else(|| format!("no modification time was kept as {name:?}"))?;
                let now = modified_nanos(&path)?;
                (now == kept)
                    .then_some(())
                    .ok_or(format!("the modification time is {now}, not {kept}"))
            }
            FsKind::Executable(wanted) => {
                let mode = fs::metadata(&path)
                    .map_err(|error| error.to_string())?
                    .permissions()
                    .mode();
                (((mode & 0o111) == 0o111) == wanted)
                    .then_some(())
                    .ok_or(format!("the mode is {mode:o}"))
            }
            FsKind::Mtime(mtime) => {
                let expected = match mtime {
                    Mtime::Epoch(seconds) => seconds,
                    Mtime::Local { local } => local_epoch(local)?,
                };
                let seconds = fs::metadata(&path)
                    .map_err(|error| error.to_string())?
                    .modified()
                    .map_err(|error| error.to_string())?
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map_err(|error| error.to_string())?
                    .as_secs();
                (i64::try_from(seconds).ok() == Some(expected))
                    .then_some(())
                    .ok_or(format!(
                        "the modification time is {seconds}, not {expected}"
                    ))
            }
            FsKind::Hex(text) => {
                fs::write(&path, hex::decode(text).map_err(|error| error.to_string())?)
                    .map_err(|error| error.to_string())
            }
            FsKind::Base64(text) => {
                fs::write(&path, gtk::glib::base64_decode(text)).map_err(|error| error.to_string())
            }
            FsKind::Append(text) => File::options()
                .append(true)
                .create(true)
                .open(&path)
                .and_then(|mut file| file.write_all(text.as_bytes()))
                .map_err(|error| error.to_string()),
            FsKind::RawPhoto => fs::write(&path, raw_photo()).map_err(|error| error.to_string()),
            FsKind::TarGz(archive) => {
                let mode =
                    u32::from_str_radix(&archive.mode, 8).map_err(|error| error.to_string())?;
                let mtime = u64::try_from(archive.mtime).map_err(|error| error.to_string())?;
                let encoder =
                    flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
                let mut builder = tar::Builder::new(encoder);
                for (name, text) in &archive.members {
                    let mut header = tar::Header::new_ustar();
                    header.set_size(text.len() as u64);
                    header.set_mode(mode);
                    header.set_mtime(mtime);
                    header.set_uid(0);
                    header.set_gid(0);
                    header.set_entry_type(tar::EntryType::Regular);
                    builder
                        .append_data(&mut header, name, text.as_bytes())
                        .map_err(|error| format!("{name}: {error}"))?;
                }
                let bytes = builder
                    .into_inner()
                    .and_then(flate2::write::GzEncoder::finish)
                    .map_err(|error| error.to_string())?;
                fs::write(&path, bytes).map_err(|error| error.to_string())
            }
            FsKind::ZipMembers(members) => {
                let file = File::open(&path).map_err(|error| error.to_string())?;
                let mut archive = zip::ZipArchive::new(file).map_err(|error| error.to_string())?;
                let names: Vec<String> = archive.file_names().map(str::to_owned).collect();
                let mut contents = Vec::new();
                for index in 0..archive.len() {
                    let mut member = archive.by_index(index).map_err(|error| error.to_string())?;
                    let mut bytes = Vec::new();
                    std::io::Read::read_to_end(&mut member, &mut bytes)
                        .map_err(|error| error.to_string())?;
                    contents.push(bytes);
                }
                archive_matches(root, members, &names, &contents)
            }
            FsKind::TarMembers(members) => {
                let file = File::open(&path).map_err(|error| error.to_string())?;
                let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
                let mut names = Vec::new();
                let mut contents = Vec::new();
                for entry in archive.entries().map_err(|error| error.to_string())? {
                    let mut entry = entry.map_err(|error| error.to_string())?;
                    names.push(
                        entry
                            .path()
                            .map_err(|error| error.to_string())?
                            .display()
                            .to_string(),
                    );
                    let mut bytes = Vec::new();
                    std::io::Read::read_to_end(&mut entry, &mut bytes)
                        .map_err(|error| error.to_string())?;
                    contents.push(bytes);
                }
                archive_matches(root, members, &names, &contents)
            }
            FsKind::IsZip(wanted) => {
                let is_zip = File::open(&path)
                    .ok()
                    .is_some_and(|file| zip::ZipArchive::new(file).is_ok());
                (is_zip == wanted)
                    .then_some(())
                    .ok_or(format!("is_zip is {is_zip}"))
            }
            FsKind::DefaultApp(default) => {
                use gtk::gio::prelude::FileExt as _;
                let content_type = gtk::gio::File::for_path(other(&default.file))
                    .query_info(
                        gtk::gio::FILE_ATTRIBUTE_STANDARD_CONTENT_TYPE,
                        gtk::gio::FileQueryInfoFlags::NONE,
                        gtk::gio::Cancellable::NONE,
                    )
                    .map_err(|error| error.to_string())?
                    .content_type()
                    .ok_or("the file has no content type")?;
                let associations = gtk::glib::KeyFile::new();
                associations
                    .load_from_file(&path, gtk::glib::KeyFileFlags::NONE)
                    .map_err(|error| error.to_string())?;
                let listed = associations
                    .string("Default Applications", &content_type)
                    .map_err(|error| format!("no default for {content_type}: {error}"))?;
                let first = listed.split(';').next().unwrap_or_default();
                (first == default.is)
                    .then_some(())
                    .ok_or(format!("the default for {content_type} is {first:?}"))
            }
            FsKind::Opened(paths) => {
                use gtk::gio::prelude::FileExt as _;
                let text = fs::read_to_string(&path).map_err(|error| error.to_string())?;
                let received: Vec<gtk::gio::File> = text
                    .lines()
                    .map(gtk::gio::File::for_commandline_arg)
                    .collect();
                let missing: Vec<&String> = paths
                    .iter()
                    .filter(|expected| {
                        let expected = gtk::gio::File::for_path(other(expected));
                        !received.iter().any(|file| file.equal(&expected))
                    })
                    .collect();
                (received.len() == paths.len() && missing.is_empty())
                    .then_some(())
                    .ok_or(format!("received {:?}", text.lines().collect::<Vec<_>>()))
            }
            FsKind::Toml(values) => {
                let parsed =
                    parsed_toml(&fs::read_to_string(&path).map_err(|error| error.to_string())?)?;
                let wrong: Vec<String> = values
                    .iter()
                    .filter(|(key, value)| {
                        key.split('.')
                            .try_fold(&parsed, |node, part| node.get(part))
                            != Some(*value)
                    })
                    .map(|(key, _)| key.clone())
                    .collect();
                wrong
                    .is_empty()
                    .then_some(())
                    .ok_or(format!("{wrong:?} differ in {parsed}"))
            }
            FsKind::TomlLike(name) => {
                let kept = state
                    .bytes
                    .get(name)
                    .ok_or_else(|| format!("no bytes were kept as {name:?}"))?;
                let kept = parsed_toml(&String::from_utf8_lossy(kept))?;
                let now =
                    parsed_toml(&fs::read_to_string(&path).map_err(|error| error.to_string())?)?;
                (now == kept)
                    .then_some(())
                    .ok_or(format!("the TOML is now {now}"))
            }
            FsKind::Json(values) | FsKind::JsonSet(values) => {
                let as_set = matches!(self.kind()?, FsKind::JsonSet(_));
                let text = fs::read_to_string(&path).map_err(|error| error.to_string())?;
                let parsed: Value =
                    serde_json::from_str(&text).map_err(|error| error.to_string())?;
                let same = |found: &Value, wanted: &Value| match (found, wanted) {
                    (Value::Array(found), Value::Array(wanted)) if as_set => {
                        found.iter().all(|item| wanted.contains(item))
                            && wanted.iter().all(|item| found.contains(item))
                    }
                    _ => found == wanted,
                };
                let wrong: Vec<&String> = values
                    .iter()
                    .filter(|(key, value)| {
                        parsed
                            .get(key.as_str())
                            .is_none_or(|found| !same(found, value))
                    })
                    .map(|(key, _)| key)
                    .collect();
                wrong
                    .is_empty()
                    .then_some(())
                    .ok_or(format!("{wrong:?} differ in {parsed}"))
            }
            FsKind::Launches(launches) => {
                let text = fs::read_to_string(&path).map_err(|error| error.to_string())?;
                let launched: Vec<(&str, &str)> = text
                    .lines()
                    .map(|line| line.split_once('\t').unwrap_or((line, "")))
                    .collect();
                let resolve =
                    |path: &str| fs::canonicalize(path).unwrap_or_else(|_| PathBuf::from(path));
                if let Some(first_dir) = &launches.first_dir {
                    let (_, arguments) = launched.first().ok_or("no launch was recorded")?;
                    let folder = resolve(arguments.strip_prefix("--dir=").unwrap_or(arguments));
                    return (folder == resolve(&other(first_dir).display().to_string()))
                        .then_some(())
                        .ok_or(format!("the terminal opened in {}", folder.display()));
                }
                let count = launches.count.unwrap_or_default();
                let within = resolve(
                    &other(launches.within.as_deref().unwrap_or_default())
                        .display()
                        .to_string(),
                );
                let (cwd, arguments) = launched.first().copied().unwrap_or_default();
                let folder = resolve(arguments.strip_prefix("--dir=").unwrap_or(arguments));
                (launched.len() == count
                    && launched.iter().all(|launch| *launch == launched[0])
                    && arguments.starts_with("--dir=")
                    && resolve(cwd) == folder
                    && folder.starts_with(&within))
                .then_some(())
                .ok_or(format!("the launches are {launched:?}"))
            }
            FsKind::ReplaceLines(replace) => {
                let mut files: Vec<PathBuf> = fs::read_dir(&path)
                    .map_err(|error| error.to_string())?
                    .flatten()
                    .map(|entry| entry.path())
                    .filter(|file| file.to_string_lossy().ends_with(&replace.suffix))
                    .collect();
                files.sort();
                for file in files {
                    contained(&file, &roots)?;
                    let text = fs::read_to_string(&file).map_err(|error| error.to_string())?;
                    let lines: Vec<&str> = text
                        .lines()
                        .map(|line| {
                            if line.starts_with(&replace.prefix) {
                                replace.with.as_str()
                            } else {
                                line
                            }
                        })
                        .collect();
                    fs::write(&file, lines.join("\n") + "\n").map_err(|error| error.to_string())?;
                }
                Ok(())
            }
            FsKind::PdfImage(image) => {
                fs::write(&path, pdf_image(image)?).map_err(|error| error.to_string())
            }
            FsKind::Magick(arguments) => magick(arguments, &path),
            FsKind::Ffmpeg(arguments) => ffmpeg(arguments, &path),
            FsKind::Zip(entries) => {
                let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
                let options = zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored);
                for (name, entry) in entries {
                    let mut entry_options = options;
                    let bytes = match entry {
                        ZipEntry::Text(text) => text.as_bytes().to_vec(),
                        ZipEntry::Image { image } => png(image)?,
                        ZipEntry::Member {
                            text,
                            date_time: [year, month, day, hour, minute, second],
                            mode,
                        } => {
                            let mode = u32::from_str_radix(mode, 8)
                                .map_err(|_| format!("{name}: mode is octal, not {mode:?}"))?;
                            let time = zip::DateTime::from_date_and_time(
                                *year,
                                u8::try_from(*month).map_err(|error| error.to_string())?,
                                u8::try_from(*day).map_err(|error| error.to_string())?,
                                u8::try_from(*hour).map_err(|error| error.to_string())?,
                                u8::try_from(*minute).map_err(|error| error.to_string())?,
                                u8::try_from(*second).map_err(|error| error.to_string())?,
                            )
                            .map_err(|error| format!("{name}: {error}"))?;
                            entry_options = entry_options
                                .last_modified_time(time)
                                .unix_permissions(mode);
                            text.as_bytes().to_vec()
                        }
                    };
                    archive
                        .start_file(name.as_str(), entry_options)
                        .and_then(|()| Ok(archive.write_all(&bytes)?))
                        .map_err(|error| format!("{name}: {error}"))?;
                }
                let archive = archive.finish().map_err(|error| error.to_string())?;
                fs::write(&path, archive.into_inner()).map_err(|error| error.to_string())
            }
            FsKind::Truncate(size) => File::create(&path)
                .and_then(|file| file.set_len(size))
                .map_err(|error| error.to_string()),
            FsKind::FromRepository(name) => {
                let source = Path::new(REPOSITORY).join(name);
                let bytes =
                    fs::read(&source).map_err(|error| format!("{}: {error}", source.display()))?;
                fs::write(&path, bytes).map_err(|error| error.to_string())
            }
            FsKind::Remove => {
                contained(&path, &state.roots)?;
                if path.is_dir() && !path.is_symlink() {
                    fs::remove_dir_all(&path)
                } else {
                    fs::remove_file(&path)
                }
                .map_err(|error| error.to_string())
            }
            FsKind::Rename(name) => {
                contained(&path, &state.roots)?;
                contained(&other(name), &state.roots)?;
                fs::rename(&path, other(name)).map_err(|error| error.to_string())
            }
            FsKind::KeepBytes(name) => {
                let bytes = fs::read(&path).map_err(|error| error.to_string())?;
                state.bytes.insert(name.to_owned(), bytes);
                Ok(())
            }
            FsKind::BytesOf(name) => {
                let kept = state
                    .bytes
                    .get(name)
                    .ok_or_else(|| format!("no bytes were kept as {name:?}"))?;
                let bytes = match fs::read(&path) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        let error = format!("cannot read the bytes: {error}");
                        state.fatal = Some(error.clone());
                        return Err(error);
                    }
                };
                (bytes == *kept)
                    .then_some(())
                    .ok_or(format!("the bytes differ from {name:?}"))
            }
            // A missing file has no lines yet; an unreadable or non-UTF-8 one is
            // an error, as `read_text()` raises.
            FsKind::Lines(count) => {
                let lines = match fs::read_to_string(&path) {
                    Ok(text) => text.lines().count(),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
                    Err(error) => {
                        let error = format!("cannot read the lines: {error}");
                        state.fatal = Some(error.clone());
                        return Err(error);
                    }
                };
                count
                    .matches(lines)
                    .then_some(())
                    .ok_or(format!("has {lines} lines"))
            }
        }
    }
}

/// A solid-colour PNG's bytes, as `Image.new("RGB", size, colour).save(...)`.
fn png(image: &Image) -> Result<Vec<u8>, String> {
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbImage::from_pixel(image.width, image.height, image::Rgb(image.rgb))
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|error| error.to_string())?;
    Ok(png.into_inner())
}

/// The bytes a path with `%XX` escapes stands for.
fn percent_decoded(path: &str) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::with_capacity(path.len());
    let mut rest = path.as_bytes();
    while let Some((&byte, after)) = rest.split_first() {
        if byte == b'%' {
            let digits = after
                .get(..2)
                .and_then(|digits| std::str::from_utf8(digits).ok())
                .and_then(|digits| u8::from_str_radix(digits, 16).ok())
                .ok_or_else(|| format!("{path:?} has a broken % escape"))?;
            bytes.push(digits);
            rest = &after[2..];
        } else {
            bytes.push(byte);
            rest = after;
        }
    }
    Ok(bytes)
}

/// Where the freedesktop cache keeps a file's thumbnail: the MD5 of its
/// URI, as `hashlib.md5(path.as_uri().encode()).hexdigest() + ".png"`.
fn cached_thumbnail(cache: &Path, source: &Path) -> PathBuf {
    let uri = file_uri(source);
    let digest = gtk::glib::compute_checksum_for_string(gtk::glib::ChecksumType::Md5, &uri)
        .map(|digest| digest.to_string())
        .unwrap_or_default();
    cache.join(format!("{digest}.png"))
}

/// `path.as_uri()`.
fn file_uri(path: &Path) -> String {
    format!("file://{}", quote_path(path.as_os_str().as_encoded_bytes()))
}

/// A solid-colour cached thumbnail for `source`, tagged with its URI and
/// whole-second modification time, written in one write.
fn write_thumbnail(thumbnail: &Thumbnail, source: &Path, roots: &[PathBuf]) -> Result<(), String> {
    let cache = Path::new(&thumbnail.cache);
    fs::create_dir_all(cache).map_err(|error| format!("{}: {error}", cache.display()))?;
    contained(cache, roots)?;
    contained(&cached_thumbnail(cache, source), roots)?;
    if let Some(mode) = &thumbnail.mode {
        let mode =
            u32::from_str_radix(mode, 8).map_err(|_| format!("mode is octal, not {mode:?}"))?;
        fs::set_permissions(cache, fs::Permissions::from_mode(mode))
            .map_err(|error| error.to_string())?;
    }
    let seconds = fs::metadata(source)
        .and_then(|metadata| metadata.modified())
        .map_err(|error| format!("{}: {error}", source.display()))?
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_secs();
    let width = i32::try_from(thumbnail.width).map_err(|error| error.to_string())?;
    let height = i32::try_from(thumbnail.height).map_err(|error| error.to_string())?;
    let pixbuf =
        gtk::gdk_pixbuf::Pixbuf::new(gtk::gdk_pixbuf::Colorspace::Rgb, false, 8, width, height)
            .ok_or("cannot make the thumbnail's pixels")?;
    let [red, green, blue] = thumbnail.rgb;
    pixbuf.fill(u32::from_be_bytes([red, green, blue, 0xff]));
    let uri = file_uri(source);
    let bytes = pixbuf
        .save_to_bufferv(
            "png",
            &[
                ("tEXt::Thumb::URI", uri.as_str()),
                ("tEXt::Thumb::MTime", &seconds.to_string()),
            ],
        )
        .map_err(|error| error.to_string())?;
    fs::write(cached_thumbnail(cache, source), bytes).map_err(|error| error.to_string())
}

/// `path.stat().st_mtime_ns`.
fn modified_nanos(path: &Path) -> Result<u128, String> {
    Ok(fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .map_err(|error| format!("{}: {error}", path.display()))?
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos())
}

/// `int(datetime(...).timestamp())`: a naive date and time read in the
/// runner's local time zone, as the twin's process reads it.
fn local_epoch([year, month, day, hour, minute, second]: [i32; 6]) -> Result<i64, String> {
    gtk::glib::DateTime::from_local(year, month, day, hour, minute, f64::from(second))
        .map(|time| time.to_unix())
        .map_err(|error| error.to_string())
}

/// Member names in order, each with the bytes of the fixture file it names.
fn archive_matches(
    root: &Path,
    members: &[(String, String)],
    names: &[String],
    contents: &[Vec<u8>],
) -> Result<(), String> {
    let expected: Vec<&String> = members.iter().map(|(name, _)| name).collect();
    if names.iter().collect::<Vec<_>>() != expected {
        return Err(format!("the members are {names:?}"));
    }
    for ((name, source), bytes) in members.iter().zip(contents) {
        let wanted = fs::read(root.join(source)).map_err(|error| format!("{source}: {error}"))?;
        if *bytes != wanted {
            return Err(format!("{name} holds other bytes than {source}"));
        }
    }
    Ok(())
}

/// A TOML document as JSON, as `tomllib.loads` reads it into a dict.
fn parsed_toml(text: &str) -> Result<Value, String> {
    let table: toml::Table = text
        .parse()
        .map_err(|error| format!("invalid TOML: {error}"))?;
    serde_json::to_value(table).map_err(|error| error.to_string())
}

/// The synthetic linear DNG `tests/e2e/harness/raw_photo.py` writes: a
/// 600x400 RGB raw with EXIF and GPS data and a 300x200 preview first.
fn raw_photo() -> Vec<u8> {
    type Tag = (u16, u16, u32, Vec<u8>);
    fn short(tag: u16, value: u16) -> Tag {
        (tag, 3, 1, value.to_le_bytes().to_vec())
    }
    fn long(tag: u16, value: u32) -> Tag {
        (tag, 4, 1, value.to_le_bytes().to_vec())
    }
    fn text(tag: u16, value: &str) -> Tag {
        let mut bytes = value.as_bytes().to_vec();
        bytes.push(0);
        (tag, 2, bytes.len() as u32, bytes)
    }
    fn rational(tag: u16, values: &[(u32, u32)]) -> Tag {
        let bytes = values
            .iter()
            .flat_map(|(numerator, denominator)| {
                [numerator.to_le_bytes(), denominator.to_le_bytes()].concat()
            })
            .collect();
        (tag, 5, values.len() as u32, bytes)
    }
    fn put(data: &mut [u8], at: usize, bytes: &[u8]) {
        data[at..at + bytes.len()].copy_from_slice(bytes);
    }
    fn directory(data: &mut Vec<u8>, mut tags: Vec<Tag>) -> u32 {
        tags.sort();
        let offset = data.len();
        data.extend((tags.len() as u16).to_le_bytes());
        data.extend(vec![0; tags.len() * 12 + 4]);
        for (index, (tag, kind, count, value)) in tags.iter().enumerate() {
            let position = offset + 2 + index * 12;
            put(data, position, &tag.to_le_bytes());
            put(data, position + 2, &kind.to_le_bytes());
            put(data, position + 4, &count.to_le_bytes());
            if value.len() <= 4 {
                put(data, position + 8, value);
            } else {
                let at = data.len() as u32;
                put(data, position + 8, &at.to_le_bytes());
                data.extend(value);
                if data.len() % 2 == 1 {
                    data.push(0);
                }
            }
        }
        offset as u32
    }
    fn pixels(data: &mut Vec<u8>, offset: u32, fields: &[Tag], width: u32, height: u32) {
        let mut tags: Vec<u16> = fields.iter().map(|(tag, ..)| *tag).collect();
        tags.sort_unstable();
        let strip = tags.iter().position(|tag| *tag == 273).unwrap_or_default();
        let at = data.len() as u32;
        put(
            data,
            offset as usize + 2 + strip * 12 + 8,
            &at.to_le_bytes(),
        );
        for y in 0..height {
            for x in 0..width {
                data.extend([(x * 255 / width) as u8, (y * 255 / height) as u8, 100]);
            }
        }
    }
    let mut data = b"II\x2a\0\0\0\0\0".to_vec();
    let exif = directory(
        &mut data,
        vec![
            rational(33434, &[(1, 250)]),
            short(34855, 400),
            rational(37386, &[(50, 1)]),
            text(42036, "Synthetic 50mm lens"),
            long(40962, 600),
            long(40963, 400),
        ],
    );
    let gps = directory(
        &mut data,
        vec![
            text(1, "S"),
            rational(2, &[(12, 1), (30, 1), (0, 1)]),
            text(3, "W"),
            rational(4, &[(45, 1), (15, 1), (0, 1)]),
        ],
    );
    let bits: Tag = (
        258,
        3,
        3,
        [8u16, 8, 8]
            .iter()
            .flat_map(|bits| bits.to_le_bytes())
            .collect(),
    );
    let version: Tag = (50706, 1, 4, vec![1, 4, 0, 0]);
    let (width, height) = (600, 400);
    let tags = vec![
        long(256, width),
        long(257, height),
        bits.clone(),
        short(259, 1),
        short(262, 34892),
        text(271, "Strata"),
        text(272, "Test Camera"),
        long(273, 0),
        short(274, 1),
        short(277, 3),
        long(278, height),
        long(279, width * height * 3),
        long(34665, exif),
        long(34853, gps),
        version.clone(),
        text(50708, "Strata Test Camera"),
    ];
    let root = directory(&mut data, tags.clone());
    pixels(&mut data, root, &tags, width, height);
    let (preview_width, preview_height) = (300, 200);
    let preview_tags = vec![
        long(254, 1),
        long(256, preview_width),
        long(257, preview_height),
        bits,
        short(259, 1),
        short(262, 2),
        text(271, "Strata"),
        text(272, "Test Camera"),
        long(273, 0),
        short(274, 1),
        short(277, 3),
        long(278, preview_height),
        long(279, preview_width * preview_height * 3),
        long(330, root),
        long(34665, exif),
        long(34853, gps),
        version,
        text(50708, "Strata Test Camera"),
    ];
    let preview = directory(&mut data, preview_tags.clone());
    put(&mut data, 4, &preview.to_le_bytes());
    pixels(
        &mut data,
        preview,
        &preview_tags,
        preview_width,
        preview_height,
    );
    data
}

/// A one-page PDF of a solid-colour image as PIL saves an RGB image to a
/// `.pdf` path: a JPEG (`DCTDecode`) image drawn over a page of the image's
/// size at 72 dpi.
fn pdf_image(image: &Image) -> Result<Vec<u8>, String> {
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 75)
        .encode_image(&image::RgbImage::from_pixel(
            image.width,
            image.height,
            image::Rgb(image.rgb),
        ))
        .map_err(|error| error.to_string())?;
    let (width, height) = (image.width, image.height);
    let contents = format!("q {width} 0 0 {height} 0 0 cm /image Do Q");
    let mut objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] \
             /Resources << /XObject << /image 4 0 R >> >> /Contents 5 0 R >>"
        )
        .into_bytes(),
    ];
    let mut image_object = format!(
        "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} \
         /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
        jpeg.len()
    )
    .into_bytes();
    image_object.extend(&jpeg);
    image_object.extend(b"\nendstream");
    objects.push(image_object);
    objects.push(
        format!(
            "<< /Length {} >>\nstream\n{contents}\nendstream",
            contents.len()
        )
        .into_bytes(),
    );
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend(format!("{} 0 obj\n", index + 1).into_bytes());
        pdf.extend(object);
        pdf.extend(b"\nendobj\n");
    }
    let table = pdf.len();
    pdf.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).into_bytes());
    for offset in offsets {
        pdf.extend(format!("{offset:010} 00000 n \n").into_bytes());
    }
    pdf.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{table}\n%%EOF\n",
            objects.len() + 1
        )
        .into_bytes(),
    );
    Ok(pdf)
}

/// `[shutil.which("magick") or shutil.which("convert"), *arguments, output]`,
/// as the HEIC fixture runs ImageMagick with a 30 s timeout.
fn magick(arguments: &[String], output: &Path) -> Result<(), String> {
    let found = |name: &str| {
        std::env::var_os("PATH")
            .into_iter()
            .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
            .map(|directory| directory.join(name))
            .find(|candidate| candidate.is_file())
    };
    let program = found("magick")
        .or_else(|| found("convert"))
        .ok_or("the pinned image must provide ImageMagick")?;
    let mut child = spawn(
        Command::new(program)
            .args(arguments)
            .arg(output)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped()),
    )?;
    let deadline = Instant::now() + FFMPEG_TIMEOUT;
    loop {
        match child.try_wait().map_err(|error| error.to_string())? {
            Some(status) if status.success() => return Ok(()),
            Some(status) => {
                let mut errors = String::new();
                if let Some(mut stderr) = child.stderr.take() {
                    let _ = std::io::Read::read_to_string(&mut stderr, &mut errors);
                }
                return Err(format!(
                    "ImageMagick exited with {status}: {}",
                    errors.trim()
                ));
            }
            None if Instant::now() >= deadline => {
                terminate(&mut child);
                return Err(format!(
                    "ImageMagick ran past {} s",
                    FFMPEG_TIMEOUT.as_secs()
                ));
            }
            None => std::thread::sleep(POLL_INTERVAL),
        }
    }
}

/// `shutil.copy2(shutil.which(name), path)`: the program's bytes, mode, and
/// access and modification times.
fn copy_program(name: &str, path: &Path) -> Result<(), String> {
    let source = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|directory| directory.join(name))
        .find(|candidate| {
            fs::metadata(candidate).is_ok_and(|metadata| {
                metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
            })
        })
        .ok_or_else(|| format!("no program {name:?} on PATH"))?;
    fs::copy(&source, path).map_err(|error| format!("{}: {error}", source.display()))?;
    let metadata = fs::metadata(&source).map_err(|error| error.to_string())?;
    let times = fs::FileTimes::new()
        .set_accessed(metadata.accessed().map_err(|error| error.to_string())?)
        .set_modified(metadata.modified().map_err(|error| error.to_string())?);
    File::options()
        .write(true)
        .open(path)
        .and_then(|file| file.set_times(times))
        .map_err(|error| error.to_string())
}

/// Sorted with hidden files, as `FixtureTree.names` lists them.
fn sorted_names(path: &Path) -> Result<Vec<String>, String> {
    let mut names: Vec<String> = fs::read_dir(path)
        .map_err(|error| error.to_string())?
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    Ok(names)
}

fn ffmpeg(arguments: &[String], output: &Path) -> Result<(), String> {
    let mut child = spawn(
        Command::new("ffmpeg")
            .args(["-v", "error", "-y"])
            .args(arguments)
            .arg(output)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped()),
    )?;
    let deadline = Instant::now() + FFMPEG_TIMEOUT;
    loop {
        match child.try_wait().map_err(|error| error.to_string())? {
            Some(status) if status.success() => return Ok(()),
            Some(status) => {
                let mut errors = String::new();
                if let Some(mut stderr) = child.stderr.take() {
                    let _ = std::io::Read::read_to_string(&mut stderr, &mut errors);
                }
                return Err(format!("ffmpeg exited with {status}: {}", errors.trim()));
            }
            None if Instant::now() >= deadline => {
                terminate(&mut child);
                return Err(format!("ffmpeg ran past {} s", FFMPEG_TIMEOUT.as_secs()));
            }
            None => std::thread::sleep(POLL_INTERVAL),
        }
    }
}

// ------------------------------------------------------------------ runs

/// `errors` are the harness's own problems (setup, launch, layouts,
/// teardown, artifacts) and always fail a run, even one marked `known_bug`;
/// `failures` are the scenario's, each with the file line of its step.
#[derive(Default)]
struct Outcome {
    errors: Vec<String>,
    failures: Vec<(Option<usize>, String)>,
    /// Where the run's artifacts went, when it wrote them.
    artifacts: Option<PathBuf>,
    last_tree: Option<String>,
    transcript: String,
}

impl Outcome {
    fn failure_texts(&self) -> impl Iterator<Item = &String> {
        self.failures.iter().map(|(_, text)| text)
    }
}

fn run_scenario(name: &str) {
    require_container();
    let path = scenario_path(name);
    let scenario =
        load_scenario(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let manifest = manifest();
    for run in runs(&scenario.header) {
        if let Err(error) = run_steps(&scenario, &run, &variables(&run, &placeholders())) {
            panic!("{}/{}: {error}", scenario.header.scenario, run.label);
        }
    }
    // Written past libtest's output capture, so the plan shows even when a run
    // never reports; `scripts/qa_parity.py` requires a result line for each.
    {
        let mut planned = std::io::stdout().lock();
        // libtest may have left `test <name> ... ` open on the current line.
        let _ = writeln!(planned);
        for run in runs(&scenario.header) {
            let _ = writeln!(
                planned,
                "RUN {}/{} (twin {})",
                scenario.header.scenario,
                run.label,
                twin(&scenario.header, &run)
            );
        }
        let _ = planned.flush();
    }
    let mut failed = Vec::new();
    for run in runs(&scenario.header) {
        let outcome = execute(&scenario, &run, &manifest);
        let name = format!(
            "{}/{} (twin {})",
            scenario.header.scenario,
            run.label,
            twin(&scenario.header, &run)
        );
        let details = |heading: &str| {
            let mut lines: Vec<String> = outcome
                .errors
                .iter()
                .chain(outcome.failure_texts())
                .cloned()
                .collect();
            if let Some(artifacts) = &outcome.artifacts {
                lines.push(format!("artifacts: {}", artifacts.display()));
            }
            format!("{name} {heading}:\n  {}", lines.join("\n  "))
        };
        if !outcome.errors.is_empty() {
            failed.push(details("could not run"));
            continue;
        }
        let header = &scenario.header;
        match (&header.known_bug, outcome.failures.is_empty()) {
            (None, true) => println!("PASS {name}"),
            (None, false) => failed.push(details("failed")),
            (Some(bug), true) => failed.push(format!(
                "{name} passes but is marked known_bug {bug}; remove the marker"
            )),
            (Some(bug), false)
                if outcome
                    .failures
                    .iter()
                    .all(|(line, _)| *line == header.known_bug_step) =>
            {
                println!("KNOWN BUG {name} still fails as expected ({bug})");
            }
            (Some(_), false) => failed.push(details(&format!(
                "is marked known_bug at line {} but failed elsewhere",
                header.known_bug_step.unwrap_or_default()
            ))),
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n\n"));
}

fn execute(scenario: &Scenario, run: &Run, manifest: &Manifest) -> Outcome {
    let scratch = tempfile::Builder::new()
        .prefix("strata-qa-")
        .tempdir_in("/tmp")
        .expect("the run's scratch directory should be created");
    let parity = std::env::var_os(PARITY_FIXTURES).is_some_and(|value| value == "1");
    let parity_name =
        |prefix: &str| parity.then(|| parity_name(prefix, &twin(&scenario.header, run)));
    let mut fixture = tempfile::Builder::new();
    let fixture = match &parity_name("strata-fixture-") {
        Some(name) => fixture.prefix(name).rand_bytes(0),
        None => fixture.prefix("strata-qa-fixture-"),
    }
    .tempdir_in("/tmp")
    .expect("the fixture directory should be created");
    // The twin's `TestEnvironment` root, which holds HOME and the XDG
    // directories, is named alike, so a typed home path is the same too.
    let parity_home = parity_name("strata-home-").map(|name| {
        tempfile::Builder::new()
            .prefix(&name)
            .rand_bytes(0)
            .tempdir_in("/tmp")
            .expect("the environment directory should be created")
    });
    let artifacts = Path::new(REPOSITORY)
        .join("target/qa-artifacts")
        .join(&scenario.header.scenario)
        .join(&run.label);
    let _ = fs::remove_dir_all(&artifacts);
    let setup_error = |error: String| Outcome {
        errors: vec![error],
        ..Outcome::default()
    };

    let home = parity_home.as_ref().map_or_else(
        || scratch.path().join("home"),
        |home| home.path().to_owned(),
    );
    let runtime = scratch.path().join("runtime");
    let places = Places {
        fixture: fixture.path(),
        scratch_home: &home,
    };
    let steps = match run_steps(scenario, run, &variables(run, &places)) {
        Ok(steps) => steps,
        Err(error) => return setup_error(format!("scenario: {error}")),
    };
    let mut fs_state = FsState {
        roots: vec![
            fixture.path().to_owned(),
            home.clone(),
            scratch.path().to_owned(),
        ],
        ..FsState::default()
    };
    // Once setup has begun, teardown runs however the run ends.
    let teardown = |fs_state: &mut FsState| -> Vec<String> {
        scenario
            .header
            .teardown
            .iter()
            .filter_map(|step| {
                let step = unescape(&substitute(step, &variables(run, &places)));
                wait_for_fs(&step, fixture.path(), fixture_time(manifest), fs_state)
                    .err()
                    .map(|error| format!("teardown: {error}"))
            })
            .collect()
    };
    let failed_setup = |error: String, fs_state: &mut FsState| {
        let mut outcome = setup_error(error);
        outcome.errors.extend(teardown(fs_state));
        outcome
    };
    if let Err(error) = seed(manifest, scenario, run, &home, &runtime, fixture.path()) {
        return failed_setup(format!("setup: {error}"), &mut fs_state);
    }
    for step in scenario.header.setup.iter().chain(&run.variant.setup) {
        let step = unescape(&substitute(step, &variables(run, &places)));
        if let Err(error) =
            wait_for_fs(&step, fixture.path(), fixture_time(manifest), &mut fs_state)
        {
            return failed_setup(format!("setup: {error}"), &mut fs_state);
        }
    }
    let display = match Display::start(scratch.path().join("xvfb.log")) {
        Ok(display) => display,
        Err(error) => return failed_setup(format!("setup: {error}"), &mut fs_state),
    };
    let bus = if scenario.header.session_bus {
        match Bus::start(&runtime) {
            Ok(bus) => Some(bus),
            Err(error) => return failed_setup(format!("setup: {error}"), &mut fs_state),
        }
    } else {
        None
    };
    let launch = Launch {
        manifest,
        home: &home,
        fixture: fixture.path(),
        display: &display,
        runtime: &runtime,
        log: scratch.path().join("strata.log"),
        input_log: &scratch.path().join(INPUT_LOG),
        extra: scenario
            .header
            .env
            .iter()
            .map(|(key, value)| {
                (
                    key.clone(),
                    unescape(&Value::String(substitute_text(
                        value,
                        &variables(run, &places),
                    )))
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                )
            })
            .collect(),
        bus: bus.as_ref().map(|bus| bus.address.as_str()),
    };
    let mut application = match launch.start() {
        Ok(application) => application,
        Err(error) => return failed_setup(format!("launch: {error}"), &mut fs_state),
    };

    let mut layout = DEFAULT_LAYOUT.to_owned();
    let mut outcome = drive(
        &mut application,
        &Context {
            steps: &steps,
            initial_layout: run
                .variant
                .layout
                .as_deref()
                .unwrap_or(&scenario.header.layout),
            manifest,
            display: &display,
            fixture: fixture.path(),
            fixture_time: fixture_time(manifest),
            screens: &scratch.path().join("screenshots"),
            launch: &launch,
        },
        &mut layout,
        &mut fs_state,
    );
    outcome.errors.extend(teardown(&mut fs_state));
    if layout != DEFAULT_LAYOUT
        && let Err(error) = display.restore_layout()
    {
        outcome.errors.push(format!("teardown: {error}"));
    }
    let failed = !outcome.errors.is_empty() || !outcome.failures.is_empty();
    let collect =
        failed || std::env::var_os("STRATA_QA_KEEP_ARTIFACTS").is_some_and(|value| value == "1");
    let mut final_state = Value::Null;
    if collect {
        // The window as the run left it, for final-state parity with the twin.
        if application.responsive {
            final_state = settled_state(&mut application);
        }
        if outcome.last_tree.is_none()
            && application.responsive
            && let Ok(reply) = application.send(&json!({ "query": "tree" }), COMMAND_TIMEOUT)
        {
            outcome.last_tree = reply["result"].as_str().map(str::to_owned);
        }
        let _ = fs::create_dir_all(&artifacts);
        if let Err(error) = display.screenshot(&artifacts.join("screenshot.png")) {
            outcome.errors.push(format!("artifacts: {error}"));
        }
    }
    application.stop();
    if collect {
        let mut files = vec![
            ("transcript.jsonl", std::mem::take(&mut outcome.transcript)),
            ("fixture.txt", listing(fixture.path())),
            (
                "strata.log",
                fs::read_to_string(&application.log).unwrap_or_default(),
            ),
            (
                "xvfb.log",
                fs::read_to_string(&display.log).unwrap_or_default(),
            ),
            (
                INPUT_LOG,
                fs::read_to_string(scratch.path().join(INPUT_LOG)).unwrap_or_default(),
            ),
            (STATE_FILE, pretty(&final_state)),
            (
                RESULT_FILE,
                pretty(&run_result(scenario, run, &steps, &outcome)),
            ),
        ];
        if failed {
            let all: Vec<String> = outcome
                .errors
                .iter()
                .chain(outcome.failure_texts())
                .cloned()
                .collect();
            files.push(("failures.txt", all.join("\n")));
        }
        if let Some(tree) = outcome.last_tree.take() {
            files.push(("tree.txt", tree));
        }
        for (file, contents) in files {
            let _ = fs::write(artifacts.join(file), contents);
        }
        for screen in fs::read_dir(scratch.path().join("screenshots"))
            .into_iter()
            .flatten()
            .flatten()
        {
            let _ = fs::copy(screen.path(), artifacts.join(screen.file_name()));
        }
        if failed {
            outcome.artifacts = Some(artifacts);
        }
    }
    outcome
}

/// `parity_fixture_name` in `tests/e2e/harness/fixtures.py`, and with
/// `strata-home-` `parity_environment_name` in `harness/environment.py`,
/// derive the same names.
fn parity_name(prefix: &str, twin: &str) -> String {
    use sha2::Digest as _;
    let digest = sha2::Sha256::digest(twin.as_bytes());
    let mut name = String::from(prefix);
    for byte in &digest[..6] {
        let _ = write!(name, "{byte:02x}");
    }
    name
}

/// The snapshot once two reads in a row agree, so an operation's late
/// selection or focus change is not compared half done; the twin waits alike.
fn settled_state(application: &mut Application) -> Value {
    let deadline = Instant::now() + STATE_SETTLE_TIMEOUT;
    let mut previous = Value::Null;
    loop {
        let Ok(reply) = application.send(&json!({ "query": "state" }), COMMAND_TIMEOUT) else {
            return previous;
        };
        let state = reply["app"].clone();
        if state == previous || Instant::now() >= deadline {
            return state;
        }
        previous = state;
        std::thread::sleep(STATE_SETTLE_INTERVAL);
    }
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default() + "\n"
}

/// What `scripts/qa_parity.py` reads back: the twin this run ports, and the
/// step each failure stopped at, with the step itself so its `message` can be
/// matched against the twin's.
fn run_result(
    scenario: &Scenario,
    run: &Run,
    steps: &[(usize, Value)],
    outcome: &Outcome,
) -> Value {
    let failures: Vec<Value> = outcome
        .failures
        .iter()
        .map(|(line, text)| {
            let step = line
                .and_then(|line| steps.iter().find(|(number, _)| *number == line))
                .map_or(Value::Null, |(_, step)| step.clone());
            json!({ "line": line, "step": step, "text": text })
        })
        .collect();
    json!({
        "scenario": scenario.header.scenario,
        "run": run.label,
        "mode": run.mode,
        "twin": twin(&scenario.header, run),
        "errors": outcome.errors,
        "failures": failures,
    })
}

fn seed(
    manifest: &Manifest,
    scenario: &Scenario,
    run: &Run,
    home: &Path,
    runtime: &Path,
    fixture: &Path,
) -> Result<(), String> {
    let io = |error: std::io::Error| error.to_string();
    for (_, directory) in XDG_DIRECTORIES {
        fs::create_dir_all(home.join(directory)).map_err(io)?;
    }
    fs::create_dir_all(runtime).map_err(io)?;
    fs::set_permissions(runtime, fs::Permissions::from_mode(0o700)).map_err(io)?;
    let gtk = home.join("config/gtk-4.0");
    fs::create_dir_all(&gtk).map_err(io)?;
    fs::write(
        gtk.join("settings.ini"),
        gtk_settings(&manifest.gtk_settings, &scenario.header.gtk_settings)?,
    )
    .map_err(io)?;
    let strata = home.join("config/strata");
    fs::create_dir_all(&strata).map_err(io)?;
    // Pre-answers the first-run portal offer, which would cover every scenario.
    fs::write(strata.join(&manifest.portal_offer_marker), "1\n").map_err(io)?;

    let mut preferences = manifest.preferences.clone();
    for (key, value) in scenario.header.prefs.iter().chain(run.variant.prefs.iter()) {
        preferences.update(key, value.clone());
    }
    preferences.update("browser_mode", Value::String(run.mode.clone()));
    fs::write(strata.join("settings.toml"), render_toml(&preferences)).map_err(io)?;

    let tree = expand_tree(
        run.variant
            .tree
            .as_ref()
            .or(scenario.header.tree.as_ref())
            .unwrap_or(&Value::String(DEFAULT_TREE.to_owned())),
        manifest,
    )?;
    populate(fixture, &tree);
    pin_times(fixture, fixture_time(manifest));
    for (path, image) in &scenario.header.images {
        image::RgbImage::from_pixel(image.width, image.height, image::Rgb(image.rgb))
            .save_with_format(fixture.join(path), image::ImageFormat::Png)
            .map_err(|error| format!("cannot write {path}: {error}"))?;
    }
    Ok(())
}

/// The manifest's `settings.ini` with each named key's line rewritten, as
/// `settings.write_text(settings.read_text().replace("key=old", "key=new"))`.
fn gtk_settings(base: &str, changes: &BTreeMap<String, String>) -> Result<String, String> {
    let mut lines: Vec<String> = base.lines().map(str::to_owned).collect();
    for (key, value) in changes {
        let line = lines
            .iter_mut()
            .find(|line| line.split_once('=').is_some_and(|(name, _)| name == key))
            .ok_or_else(|| format!("the manifest's GTK settings have no {key}"))?;
        *line = format!("{key}={value}");
    }
    let mut text = lines.join("\n");
    if base.ends_with('\n') {
        text.push('\n');
    }
    Ok(text)
}

/// A header `tree`: a manifest tree's name, or a tree in the manifest's
/// shape where a name like `{000..199}.txt` stands for one file per number,
/// zero-padded to the width written, whose text has `{}` replaced by the
/// number, as a Python fixture's comprehension writes them.
fn expand_tree(tree: &Value, manifest: &Manifest) -> Result<Value, String> {
    match tree {
        Value::String(name) => manifest
            .trees
            .get(name)
            .cloned()
            .ok_or_else(|| format!("no fixture tree {name:?} in the manifest")),
        Value::Object(_) => expand_directory(tree),
        other => Err(format!(
            "tree is a manifest tree name or a tree, not {other}"
        )),
    }
}

fn expand_directory(directory: &Value) -> Result<Value, String> {
    let Value::Object(entries) = directory else {
        return Err("a fixture directory must be an object".to_owned());
    };
    let mut expanded = Map::new();
    for (name, value) in entries {
        let child = match value {
            Value::Object(_) => expand_directory(value)?,
            Value::String(_) => value.clone(),
            Value::Array(parts) => Value::String(file_parts(name, parts)?),
            _ => {
                return Err(format!(
                    "fixture {name} must be a file's text or a directory"
                ));
            }
        };
        for (expanded_name, number) in numbered(name)? {
            let contents = match (&child, number) {
                (Value::String(text), Some(number)) => {
                    Value::String(text.replace("{}", &number.to_string()))
                }
                (other, _) => other.clone(),
            };
            expanded.insert(expanded_name, contents);
        }
    }
    Ok(Value::Object(expanded))
}

/// A file's text written in parts: each a string, or `{"repeat":"record-{},{}\n",
/// "from":0,"to":999}`, the text once per number with `{}` replaced by it, as
/// a fixture joins `f"record-{index},{index}\n" for index in range(1000)`.
fn file_parts(name: &str, parts: &[Value]) -> Result<String, String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Repeat {
        repeat: String,
        from: u32,
        to: u32,
    }
    let mut text = String::new();
    for part in parts {
        match part {
            Value::String(part) => text.push_str(part),
            other => {
                let repeat: Repeat = serde_json::from_value(other.clone()).map_err(|error| {
                    format!("fixture {name}: a part is text or a repeat: {error}")
                })?;
                for number in repeat.from..=repeat.to {
                    text.push_str(&repeat.repeat.replace("{}", &number.to_string()));
                }
            }
        }
    }
    Ok(text)
}

/// A name, or the names a `{000..199}` range in it stands for.
fn expand_name(name: &str) -> Result<Vec<String>, String> {
    Ok(numbered(name)?.into_iter().map(|(name, _)| name).collect())
}

/// Each name a `{first..last}` range stands for, zero-padded to the width
/// `first` is written in, with its number; a name without one stands alone.
fn numbered(name: &str) -> Result<Vec<(String, Option<u32>)>, String> {
    let range = name.split_once('{').and_then(|(prefix, rest)| {
        let (range, suffix) = rest.split_once('}')?;
        let (first, last) = range.split_once("..")?;
        Some((prefix, first, last.parse::<u32>().ok()?, suffix))
    });
    let Some((prefix, first, last, suffix)) = range else {
        return Ok(vec![(name.to_owned(), None)]);
    };
    let start: u32 = first
        .parse()
        .map_err(|_| format!("fixture {name}: the range must be numeric"))?;
    Ok((start..=last)
        .map(|number| {
            (
                format!("{prefix}{number:0width$}{suffix}", width = first.len()),
                Some(number),
            )
        })
        .collect())
}

fn fixture_time(manifest: &Manifest) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(manifest.mtime.unsigned_abs())
}

fn switch(manifest: &Manifest, display: &Display, name: &str) -> Result<(), String> {
    if name == DEFAULT_LAYOUT {
        return display.restore_layout();
    }
    let layout = manifest
        .layouts
        .get(name)
        .ok_or_else(|| format!("no layout {name:?} in the manifest"))?;
    display.switch_layout(layout)
}

struct Context<'a> {
    /// With their file lines.
    steps: &'a [(usize, Value)],
    initial_layout: &'a str,
    manifest: &'a Manifest,
    display: &'a Display,
    fixture: &'a Path,
    /// The manifest's pinned time, which an fs `populate` gives its tree.
    fixture_time: SystemTime,
    /// Where `screenshot` steps save, copied into the artifacts when a run keeps them.
    screens: &'a Path,
    /// How the application started, for `restart`, `dbus_send`, `log`, and `proc`.
    launch: &'a Launch<'a>,
}

fn drive(
    application: &mut Application,
    context: &Context,
    layout: &mut String,
    fs_state: &mut FsState,
) -> Outcome {
    let mut outcome = Outcome::default();
    let hello = application.receive(LAUNCH_TIMEOUT);
    if let Err(error) = hello.and_then(|hello| {
        record(
            &mut outcome,
            None,
            "launch",
            &json!({ "launch": true }),
            hello,
        )
    }) {
        outcome.errors.push(format!("launch: {error}"));
        return outcome;
    }
    // Only now: the process read the US keymap at startup, and key names
    // keep meaning US key positions under any layout.
    if context.initial_layout != DEFAULT_LAYOUT {
        match switch(context.manifest, context.display, context.initial_layout) {
            Ok(()) => context.initial_layout.clone_into(layout),
            Err(error) => {
                outcome.errors.push(format!("layout: {error}"));
                return outcome;
            }
        }
    }
    let mut notes: Vec<(String, Value)> = Vec::new();
    let mut flags: BTreeMap<String, bool> = BTreeMap::new();
    for (line, step) in context.steps {
        let step = &unescape(&substitute_notes(step, &notes));
        let label = format!("line {line} {step}");
        let mut object = step.as_object().cloned().unwrap_or_default();
        let held = |key: &str, object: &mut Map<String, Value>| {
            object
                .remove(key)
                .and_then(|flag| flag.as_str().and_then(|flag| flags.get(flag).copied()))
        };
        if held(STEP_IF, &mut object) == Some(false) || held(STEP_UNLESS, &mut object) == Some(true)
        {
            continue;
        }
        if let Some(condition) = object.remove(STEP_WHEN) {
            let probe = json!({ "holds": condition });
            match application.send(&probe, COMMAND_TIMEOUT) {
                Ok(reply) => {
                    let holds = reply["result"] == json!(true);
                    if let Err(error) = record(&mut outcome, Some(*line), &label, &probe, reply) {
                        file_failure(&mut outcome, *line, &label, &error);
                        return outcome;
                    }
                    if !holds {
                        continue;
                    }
                }
                Err(error) => {
                    outcome.errors.push(format!("{label}: {error}"));
                    return outcome;
                }
            }
        }
        let step = &Value::Object(object.clone());
        if let Some(name) = object.get("layout") {
            let name = name.as_str().unwrap_or_default().to_owned();
            match switch(context.manifest, context.display, &name) {
                Ok(()) => *layout = name,
                Err(error) => {
                    outcome.errors.push(format!("{label}: {error}"));
                    return outcome;
                }
            }
            continue;
        }
        if let Some(condition) = object.get("flag") {
            let probe = json!({ "holds": condition });
            match application.send(&probe, COMMAND_TIMEOUT) {
                Ok(reply) => {
                    let holds = reply["result"] == json!(true);
                    if let Err(error) = record(&mut outcome, Some(*line), &label, &probe, reply) {
                        file_failure(&mut outcome, *line, &label, &error);
                        return outcome;
                    }
                    flags.insert(object["as"].as_str().unwrap_or_default().to_owned(), holds);
                }
                Err(error) => {
                    outcome.errors.push(format!("{label}: {error}"));
                    return outcome;
                }
            }
            continue;
        }
        if let Some(name) = object.get("screenshot") {
            let destination = context.screens.join(name.as_str().unwrap_or_default());
            let taken = fs::create_dir_all(context.screens)
                .map_err(|error| error.to_string())
                .and_then(|()| context.display.screenshot(&destination));
            if let Err(error) = taken {
                outcome.errors.push(format!("{label}: {error}"));
                return outcome;
            }
            continue;
        }
        if let Some(source) = object.get("each") {
            let each: EachStep = match serde_json::from_value(step.clone()) {
                Ok(each) => each,
                Err(error) => {
                    outcome
                        .errors
                        .push(format!("{label}: invalid each step: {error}"));
                    return outcome;
                }
            };
            let items = notes
                .iter()
                .find(|(note, _)| Some(note.as_str()) == source.as_str())
                .and_then(|(_, value)| value.as_array().cloned())
                .unwrap_or_default();
            for item in items {
                let inner = match each.expand(&item) {
                    Ok(inner) => unescape(&inner),
                    Err(error) => {
                        file_failure(&mut outcome, *line, &label, &error);
                        return outcome;
                    }
                };
                let timeout = match inner.get("timeout_ms").and_then(Value::as_u64) {
                    Some(ms) => (Duration::from_millis(ms) + WAIT_MARGIN).max(COMMAND_TIMEOUT),
                    None => COMMAND_TIMEOUT,
                };
                let inner_label = format!("line {line} {inner}");
                match application.send(&inner, timeout) {
                    Ok(reply) => {
                        if let Err(error) =
                            record(&mut outcome, Some(*line), &inner_label, &inner, reply)
                        {
                            file_failure(&mut outcome, *line, &inner_label, &error);
                            return outcome;
                        }
                    }
                    Err(error) => {
                        outcome.errors.push(format!("{inner_label}: {error}"));
                        return outcome;
                    }
                }
            }
            continue;
        }
        if object.contains_key("restart") {
            if *layout != DEFAULT_LAYOUT {
                outcome.errors.push(format!(
                    "{label}: a restart reads the keymap again, so it needs the US layout"
                ));
                return outcome;
            }
            application.terminate_group();
            let started = context.launch.start().and_then(|started| {
                *application = started;
                application.receive(LAUNCH_TIMEOUT)
            });
            if let Err(error) = started.and_then(|hello| {
                record(
                    &mut outcome,
                    Some(*line),
                    &label,
                    &json!({ "launch": true }),
                    hello,
                )
            }) {
                outcome.errors.push(format!("{label}: {error}"));
                return outcome;
            }
            continue;
        }
        if let Some(arguments) = object.get("dbus_send") {
            let arguments: Vec<String> = arguments
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|argument| argument.as_str().map(str::to_owned))
                .collect();
            if let Err(error) = dbus_send(context.launch, &arguments) {
                file_failure(&mut outcome, *line, &label, &error);
                return outcome;
            }
            continue;
        }
        if object.contains_key("log") {
            let parsed: LogStep = match serde_json::from_value(step.clone()) {
                Ok(parsed) => parsed,
                Err(error) => {
                    outcome
                        .errors
                        .push(format!("{label}: invalid log step: {error}"));
                    return outcome;
                }
            };
            match parsed.run(&application.log) {
                Ok(Kept::Nothing) => {}
                Ok(Kept::Flag(name, holds)) => {
                    flags.insert(name, holds);
                }
                Ok(Kept::Note(name, value)) => {
                    notes.retain(|(note, _)| *note != name);
                    notes.push((name, value));
                }
                Err(error) => {
                    file_failure(&mut outcome, *line, &label, &error);
                    return outcome;
                }
            }
            continue;
        }
        if object.contains_key("proc") {
            let parsed: ProcStep = match serde_json::from_value(step.clone()) {
                Ok(parsed) => parsed,
                Err(error) => {
                    outcome
                        .errors
                        .push(format!("{label}: invalid proc step: {error}"));
                    return outcome;
                }
            };
            if let Err(error) = parsed.run(application.child.id()) {
                file_failure(&mut outcome, *line, &label, &error);
                return outcome;
            }
            continue;
        }
        if let Some(source) = object.get("slice") {
            let name = object["as"].as_str().unwrap_or_default().to_owned();
            let sliced = notes
                .iter()
                .find(|(note, _)| Some(note.as_str()) == source.as_str())
                .and_then(|(_, value)| value.as_array())
                .ok_or_else(|| format!("no note of a list called {source}"))
                .and_then(|names| {
                    slice(
                        names,
                        object.get("middle").and_then(Value::as_u64),
                        object.get("pick").and_then(Value::as_i64),
                    )
                });
            match sliced {
                Ok(value) => {
                    notes.retain(|(note, _)| *note != name);
                    notes.push((name, value));
                    continue;
                }
                Err(error) => {
                    file_failure(&mut outcome, *line, &label, &error);
                    return outcome;
                }
            }
        }
        if let Some((source, derive)) = object
            .get("increment")
            .map(|source| (source, increment as fn(&str) -> Result<String, String>))
            .or_else(|| object.get("stem").map(|source| (source, stem as _)))
        {
            let name = object["as"].as_str().unwrap_or_default().to_owned();
            let noted = notes
                .iter()
                .find(|(note, _)| Some(note.as_str()) == source.as_str())
                .and_then(|(_, value)| value.as_str())
                .ok_or_else(|| format!("no note of a name called {source}"))
                .and_then(derive);
            match noted {
                Ok(text) => {
                    notes.retain(|(note, _)| *note != name);
                    notes.push((name, json!(text)));
                    continue;
                }
                Err(error) => {
                    file_failure(&mut outcome, *line, &label, &error);
                    return outcome;
                }
            }
        }
        let result = if object.contains_key("fs") {
            wait_for_fs(step, context.fixture, context.fixture_time, fs_state)
        } else {
            let timeout = match object.get("timeout_ms").and_then(Value::as_u64) {
                Some(ms) => (Duration::from_millis(ms) + WAIT_MARGIN).max(COMMAND_TIMEOUT),
                None => COMMAND_TIMEOUT,
            };
            // No reply at all is the harness failing, not the scenario.
            match application.send(step, timeout) {
                Ok(reply) => {
                    let result = reply["result"].clone();
                    if let Some(name) = object.get("expect").and(object.get("as")) {
                        flags.insert(
                            name.as_str().unwrap_or_default().to_owned(),
                            reply["ok"] == json!(true),
                        );
                    }
                    let recorded = record(&mut outcome, Some(*line), &label, step, reply);
                    if recorded.is_ok()
                        && let Some(name) = object.get("note").and(object.get("as"))
                    {
                        let name = name.as_str().unwrap_or_default().to_owned();
                        notes.retain(|(note, _)| *note != name);
                        notes.push((name, result));
                    }
                    recorded
                }
                Err(error) => {
                    outcome.errors.push(format!("{label}: {error}"));
                    return outcome;
                }
            }
        };
        if let Err(error) = result {
            file_failure(&mut outcome, *line, &label, &error);
            return outcome;
        }
    }
    outcome
}

/// A step that went wrong is the scenario's failure, unless the controller
/// panicked: that is the harness's error, which no `known_bug` excuses.
fn file_failure(outcome: &mut Outcome, line: usize, label: &str, error: &str) {
    let failure = format!("{label}: {error}");
    if error.contains(CONTROLLER_PANIC) {
        outcome.errors.push(failure);
    } else {
        outcome.failures.push((Some(line), failure));
    }
}

/// Logs the exchange, keeps expect mismatches and criticals as failures, and
/// stops the run on anything else that went wrong.
fn record(
    outcome: &mut Outcome,
    line: Option<usize>,
    label: &str,
    step: &Value,
    reply: Value,
) -> Result<(), String> {
    let _ = writeln!(
        outcome.transcript,
        "{}",
        json!({ "command": step, "reply": reply })
    );
    if let Some(tree) = reply["result"]["tree"].as_str() {
        outcome.last_tree = Some(tree.to_owned());
    }
    for critical in reply["criticals"].as_array().into_iter().flatten() {
        outcome.failures.push((
            line,
            format!(
                "{label}: GLib warning or worse: {}",
                critical.as_str().unwrap_or_default()
            ),
        ));
    }
    if reply["ok"].as_bool() == Some(true) {
        return Ok(());
    }
    let failures = reply["failures"].as_array().cloned().unwrap_or_default();
    let explanation = failures
        .iter()
        .map(|failure| {
            format!(
                "{} expected {}, actual {}",
                failure["message"].as_str().unwrap_or("predicate"),
                failure["predicate"],
                failure["actual"]
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    match (reply["error"].as_str(), step.get("expect")) {
        (None, Some(_)) => {
            outcome
                .failures
                .push((line, format!("{label}: {explanation}")));
            Ok(())
        }
        (error, _) => Err(format!(
            "{}{}{explanation}",
            error.unwrap_or_default(),
            if explanation.is_empty() { "" } else { ": " }
        )),
    }
}

/// Steps were checked when the run's steps were built, so a bad body here
/// would be a runner bug. An action runs once; a check is polled.
fn wait_for_fs(
    step: &Value,
    fixture: &Path,
    fixture_time: SystemTime,
    state: &mut FsState,
) -> Result<(), String> {
    let step: FsStep = serde_json::from_value(step.clone())
        .map_err(|error| format!("invalid fs step: {error}"))?;
    if step.fs.acts() {
        return step
            .fs
            .observe(fixture, fixture_time, state)
            .map_err(|error| format!("{}: {error}", step.fs.path));
    }
    let deadline = Instant::now() + step.timeout_ms.map_or(DEFAULT_WAIT, Duration::from_millis);
    loop {
        match step.fs.observe(fixture, fixture_time, state) {
            Ok(()) => return Ok(()),
            Err(_) if state.fatal.is_some() => {
                return Err(format!(
                    "{} {}",
                    step.fs.path,
                    state.fatal.take().unwrap_or_default()
                ));
            }
            Err(actual) if Instant::now() >= deadline => {
                return Err(format!(
                    "timed out{}; {} {actual}",
                    step.message
                        .as_deref()
                        .map(|message| format!(" waiting for {message}"))
                        .unwrap_or_default(),
                    step.fs.path
                ));
            }
            Err(_) => std::thread::sleep(POLL_INTERVAL),
        }
    }
}

/// `dbus-send --session <arguments>` against the run's private bus, with
/// the environment `test_file_manager_interface.call_file_manager` builds,
/// failing as `subprocess.run(..., check=True, timeout=30)` does.
fn dbus_send(launch: &Launch, arguments: &[String]) -> Result<(), String> {
    if launch.bus.is_none() {
        return Err("dbus_send needs the header's session_bus".to_owned());
    }
    let mut child = spawn(
        Command::new("dbus-send")
            .arg("--session")
            .args(arguments)
            .env_clear()
            .envs(launch.helper_environment())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped()),
    )
    .map_err(|error| format!("cannot run dbus-send: {error}"))?;
    let deadline = Instant::now() + FFMPEG_TIMEOUT;
    loop {
        match child.try_wait().map_err(|error| error.to_string())? {
            Some(status) if status.success() => return Ok(()),
            Some(status) => {
                let mut errors = String::new();
                if let Some(mut stderr) = child.stderr.take() {
                    let _ = std::io::Read::read_to_string(&mut stderr, &mut errors);
                }
                return Err(format!("dbus-send exited with {status}: {}", errors.trim()));
            }
            None if Instant::now() >= deadline => {
                terminate(&mut child);
                return Err(format!("dbus-send ran past {} s", FFMPEG_TIMEOUT.as_secs()));
            }
            None => std::thread::sleep(POLL_INTERVAL),
        }
    }
}

/// One process step run once per item of a noted list, as a twin loops
/// `for name in names: strata.wait(...)`: `$<as>` in `step` is the item, and
/// each `derive` variable is the last number in the item's stem plus
/// `number_plus`, between `prefix` and `suffix`, as
/// `f"{320 + int(Path(name).stem.removeprefix('photo-'))}×180"`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EachStep {
    #[serde(rename = "each")]
    _source: String,
    #[serde(rename = "as")]
    name: String,
    step: Value,
    #[serde(default)]
    derive: BTreeMap<String, Derived>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Derived {
    number_plus: i64,
    #[serde(default)]
    prefix: String,
    #[serde(default)]
    suffix: String,
}

impl EachStep {
    /// The step for one item; a stand-in item derives stand-in text.
    fn expand(&self, item: &Value) -> Result<Value, String> {
        let text = match item {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        };
        let mut variables = vec![(self.name.clone(), text.clone())];
        for (key, derived) in &self.derive {
            let value = if text == NOTED_PLACEHOLDER {
                NOTED_PLACEHOLDER.to_owned()
            } else {
                let stem = stem(&text)?;
                let digits: String = stem
                    .chars()
                    .rev()
                    .take_while(char::is_ascii_digit)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                let number: i64 = digits
                    .parse()
                    .map_err(|_| format!("{text:?} has no number in its stem"))?;
                format!(
                    "{}{}{}",
                    derived.prefix,
                    number + derived.number_plus,
                    derived.suffix
                )
            };
            variables.push((key.clone(), value));
        }
        variables.sort_by_key(|(name, _)| std::cmp::Reverse(name.len()));
        if !self.step.is_object() {
            return Err("each repeats one step object".to_owned());
        }
        Ok(substitute(&self.step, &variables))
    }
}

/// A step reading the application's own log, as a twin reads
/// `strata.application.log()` with its colour codes stripped: how often a
/// line occurs (`count`), the numbers a pattern's first group finds
/// (`matches`, as `re.findall`), or whether it mentions something
/// (`contains`). With `as` it keeps the count or the numbers (one of them
/// with `pick`) as a note, or whether it mentions it as a flag; without, it
/// waits until the count, or how many numbers there are, is `equals`,
/// `at_least`, or `at_most` a number or a noted list's length, plus `plus`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LogStep {
    log: LogQuery,
    timeout_ms: Option<u64>,
    message: Option<String>,
    #[serde(rename = "as")]
    name: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LogQuery {
    count: Option<String>,
    matches: Option<String>,
    contains: Option<String>,
    pick: Option<i64>,
    equals: Option<Value>,
    at_least: Option<Value>,
    at_most: Option<Value>,
    #[serde(default)]
    plus: i64,
}

/// What a `log` step keeps for later steps.
enum Kept {
    Nothing,
    Flag(String, bool),
    Note(String, Value),
}

impl LogStep {
    /// The shape rules, checked before the run starts.
    fn check(&self) -> Result<(), String> {
        let query = &self.log;
        let kinds = [&query.count, &query.matches, &query.contains]
            .into_iter()
            .filter(|kind| kind.is_some())
            .count();
        if kinds != 1 {
            return Err("log reads exactly one of count, matches, and contains".to_owned());
        }
        if let Some(pattern) = &query.matches {
            regex::Regex::new(pattern).map_err(|error| format!("log matches: {error}"))?;
        }
        let conditions = [&query.equals, &query.at_least, &query.at_most]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        if conditions
            .iter()
            .any(|value| !value.is_u64() && !value.is_array())
        {
            return Err("a log bound is a number or a noted list".to_owned());
        }
        match &self.name {
            Some(name) if name.is_empty() => Err("log keeps under a name in as".to_owned()),
            Some(_) => {
                if !conditions.is_empty()
                    || query.plus != 0
                    || self.timeout_ms.is_some()
                    || self.message.is_some()
                {
                    return Err("a log that keeps a value checks nothing".to_owned());
                }
                if query.pick.is_some() && query.matches.is_none() {
                    return Err("pick belongs to matches".to_owned());
                }
                Ok(())
            }
            None => {
                if query.contains.is_some() || query.pick.is_some() {
                    return Err("contains and pick keep a flag or a note under as".to_owned());
                }
                if conditions.is_empty() {
                    return Err("log needs equals, at_least, or at_most".to_owned());
                }
                Ok(())
            }
        }
    }

    fn read(path: &Path) -> String {
        let text = String::from_utf8_lossy(&fs::read(path).unwrap_or_default()).into_owned();
        regex::Regex::new("\x1b\\[[0-9;]*m")
            .map(|colours| colours.replace_all(&text, "").into_owned())
            .unwrap_or(text)
    }

    fn numbers(pattern: &str, text: &str) -> Result<Vec<i64>, String> {
        let pattern = regex::Regex::new(pattern).map_err(|error| error.to_string())?;
        pattern
            .captures_iter(text)
            .map(|found| {
                found
                    .get(1)
                    .unwrap_or_else(|| found.get(0).expect("a match has its whole text"))
                    .as_str()
                    .parse::<i64>()
                    .map_err(|error| format!("log matches a non-number: {error}"))
            })
            .collect()
    }

    fn run(&self, path: &Path) -> Result<Kept, String> {
        self.check()?;
        let query = &self.log;
        if let Some(name) = &self.name {
            let text = Self::read(path);
            return Ok(match (&query.count, &query.matches, &query.contains) {
                (Some(part), _, _) => {
                    Kept::Note(name.clone(), json!(text.matches(part.as_str()).count()))
                }
                (_, Some(pattern), _) => {
                    let numbers = Self::numbers(pattern, &text)?;
                    let value = match query.pick {
                        None => json!(numbers),
                        Some(pick) => {
                            let length =
                                i64::try_from(numbers.len()).map_err(|error| error.to_string())?;
                            let index = if pick < 0 { length + pick } else { pick };
                            json!(
                                usize::try_from(index)
                                    .ok()
                                    .and_then(|index| numbers.get(index))
                                    .ok_or_else(|| format!(
                                        "index {pick} is outside {numbers:?}"
                                    ))?
                            )
                        }
                    };
                    Kept::Note(name.clone(), value)
                }
                (_, _, Some(part)) => Kept::Flag(name.clone(), text.contains(part.as_str())),
                _ => Kept::Nothing,
            });
        }
        let bound = |value: &Option<Value>| -> Option<i64> {
            value.as_ref().map(|value| {
                value.as_array().map_or_else(
                    || value.as_i64().unwrap_or_default(),
                    |items| items.len() as i64,
                ) + query.plus
            })
        };
        let (equals, at_least, at_most) = (
            bound(&query.equals),
            bound(&query.at_least),
            bound(&query.at_most),
        );
        let deadline = Instant::now() + self.timeout_ms.map_or(DEFAULT_WAIT, Duration::from_millis);
        loop {
            let text = Self::read(path);
            let observed = match (&query.count, &query.matches) {
                (Some(part), _) => text.matches(part.as_str()).count() as i64,
                (_, Some(pattern)) => Self::numbers(pattern, &text)?.len() as i64,
                _ => 0,
            };
            if equals.is_none_or(|equals| observed == equals)
                && at_least.is_none_or(|least| observed >= least)
                && at_most.is_none_or(|most| observed <= most)
            {
                return Ok(Kept::Nothing);
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "{}the log has {observed}, not {}{}{}",
                    self.message
                        .as_deref()
                        .map(|message| format!("timed out waiting for {message}; "))
                        .unwrap_or_default(),
                    equals.map(|n| format!("= {n} ")).unwrap_or_default(),
                    at_least.map(|n| format!(">= {n} ")).unwrap_or_default(),
                    at_most.map(|n| format!("<= {n}")).unwrap_or_default(),
                ));
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }
}

/// A step about processes, as a twin reads `/proc` and signals a worker:
/// waits until none of the noted pids is still running (`gone`), checks a
/// pid's parent is the application (`parent_is_app`, `PPid:` in its
/// `status`), or sends it SIGKILL at once (`kill`), which refuses any pid
/// that is not the application's child.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProcStep {
    proc: ProcQuery,
    timeout_ms: Option<u64>,
    message: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProcQuery {
    gone: Option<Value>,
    parent_is_app: Option<Value>,
    kill: Option<Value>,
}

impl ProcStep {
    fn check(&self) -> Result<(), String> {
        let query = &self.proc;
        match (&query.gone, &query.parent_is_app, &query.kill) {
            (Some(pids), None, None) if pids.is_array() => Ok(()),
            (None, Some(pid), None) if pid.is_u64() => Ok(()),
            (None, None, Some(pid)) if pid.is_u64() => {
                if self.timeout_ms.is_some() || self.message.is_some() {
                    Err("a kill happens at once; it takes no timeout_ms or message".to_owned())
                } else {
                    Ok(())
                }
            }
            _ => Err(
                "proc takes one of gone (a noted list of pids), parent_is_app, or kill (a pid)"
                    .to_owned(),
            ),
        }
    }

    fn run(&self, application: u32) -> Result<(), String> {
        let query = &self.proc;
        let pid = |value: &Value| {
            value
                .as_u64()
                .and_then(|pid| i32::try_from(pid).ok())
                .ok_or_else(|| format!("{value} is not a pid"))
        };
        let parent_is_app = |target: i32| -> Result<(), String> {
            let status = fs::read_to_string(format!("/proc/{target}/status"))
                .map_err(|error| format!("/proc/{target}/status: {error}"))?;
            if status.contains(&format!("PPid:\t{application}\n")) {
                Ok(())
            } else {
                Err(format!("{target} is not a child of {application}"))
            }
        };
        if let Some(target) = &query.kill {
            // Only the application's own children, never anything else.
            let raw = pid(target)?;
            parent_is_app(raw).map_err(|error| format!("refusing to kill: {error}"))?;
            let target = rustix::process::Pid::from_raw(raw)
                .ok_or_else(|| format!("{target} is not a pid"))?;
            return rustix::process::kill_process(target, rustix::process::Signal::KILL)
                .map_err(|error| format!("cannot kill {target:?}: {error}"));
        }
        let deadline = Instant::now() + self.timeout_ms.map_or(DEFAULT_WAIT, Duration::from_millis);
        loop {
            let verdict = if let Some(pids) = &query.gone {
                let running: Vec<i32> = pids
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(pid)
                    .collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .filter(|pid| Path::new(&format!("/proc/{pid}")).exists())
                    .collect();
                if running.is_empty() {
                    Ok(())
                } else {
                    Err(format!("{running:?} still run"))
                }
            } else {
                parent_is_app(pid(query.parent_is_app.as_ref().unwrap_or(&Value::Null))?)
            };
            match verdict {
                Ok(()) => return Ok(()),
                Err(actual) if Instant::now() >= deadline => {
                    return Err(format!(
                        "{}{actual}",
                        self.message
                            .as_deref()
                            .map(|message| format!("timed out waiting for {message}; "))
                            .unwrap_or_default()
                    ));
                }
                Err(_) => std::thread::sleep(POLL_INTERVAL),
            }
        }
    }
}

// ------------------------------------------------------- scenario checks

#[test]
fn every_scenario_file_is_registered_and_well_formed() {
    let mut files: Vec<String> = scenario_files()
        .into_iter()
        .map(|path| {
            load_scenario(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            let stem = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or_default();
            stem.replace('-', "_")
        })
        .collect();
    files.sort();
    let mut registered: Vec<String> = REGISTERED.iter().map(|name| (*name).to_owned()).collect();
    registered.sort();
    assert_eq!(
        files, registered,
        "register each scenario file in scenarios! {{}}"
    );
    let manifest = manifest();
    let mut twins = BTreeMap::new();
    for name in REGISTERED {
        let path = scenario_path(name);
        let folder = path
            .parent()
            .and_then(Path::file_name)
            .and_then(|folder| folder.to_str())
            .unwrap_or_default()
            .to_owned();
        let scenario = load_scenario(&path).expect("the scenario should load");
        let header = &scenario.header;
        for run in runs(header) {
            let node = twin(header, &run);
            let (file, test) = node.split_once("::").unwrap_or((&node, ""));
            assert_eq!(
                Path::new(file).file_stem().and_then(|stem| stem.to_str()),
                Some(folder.as_str()),
                "{name}/{}: the scenario belongs in the folder named after {file}",
                run.label
            );
            let function = test.split('[').next().unwrap_or_default();
            let source = fs::read_to_string(Path::new(REPOSITORY).join(file)).unwrap_or_default();
            assert!(
                !function.is_empty() && source.contains(&format!("def {function}(")),
                "{name}/{}: the twin {node} should name a test function in its file",
                run.label
            );
            assert!(
                !node.contains('$'),
                "{name}/{}: the twin {node} has an unknown variable",
                run.label
            );
            if let Some(other) = twins.insert(node.clone(), format!("{name}/{}", run.label)) {
                panic!(
                    "{name}/{} and {other} both claim the twin {node}",
                    run.label
                );
            }
        }
        if let Some(mutation) = &header.mutation {
            assert!(
                Path::new(REPOSITORY)
                    .join(format!("tests/e2e/mutations/{mutation}.patch"))
                    .is_file(),
                "{name}: the mutation {mutation} should exist"
            );
        }
        for run in runs(header) {
            let layout = run.variant.layout.as_ref().unwrap_or(&header.layout);
            assert!(
                manifest.layouts.contains_key(layout),
                "{name}/{}: no layout {layout:?}",
                run.label
            );
            let steps = run_steps(&scenario, &run, &variables(&run, &placeholders()))
                .unwrap_or_else(|error| panic!("{name}/{}: {error}", run.label));
            assert!(!steps.is_empty(), "{name}/{}: no steps run", run.label);
        }
    }
}

#[test]
fn a_numbered_tree_expands_like_the_python_comprehension() {
    let tree =
        json!({ "nested": { "{000..002}.txt": "{}\n" }, "{8..10}.md": "n{}", "a.txt": "{}" });
    assert_eq!(
        expand_tree(&tree, &manifest()),
        Ok(json!({
            "nested": { "000.txt": "0\n", "001.txt": "1\n", "002.txt": "2\n" },
            "8.md": "n8", "9.md": "n9", "10.md": "n10",
            "a.txt": "{}",
        }))
    );
    assert!(expand_tree(&json!("missing"), &manifest()).is_err());
}

#[test]
fn a_run_keeps_its_own_mode_steps_and_rejects_a_malformed_one() {
    let header = || -> Header {
        serde_json::from_value(json!({
            "scenario": "example", "modes": ["columns", "icons"], "twin": "x",
            "mutation": null, "known_bug": null,
        }))
        .expect("the header should parse")
    };
    let scenario = |steps: Vec<Value>| Scenario {
        header: header(),
        steps: steps.into_iter().enumerate().collect(),
    };
    let steps = vec![
        json!({ "select": { "entry": "archive" }, "modes": ["columns"] }),
        json!({ "click": { "entry": "archive" }, "count": 2, "modes": ["icons"] }),
        json!({ "wait": { "view_mode": "$view" } }),
        json!({ "key": "Escape", "when": { "dialog": true } }),
    ];
    let runs = runs(&header());
    let icons = runs
        .iter()
        .find(|run| run.mode == "icons")
        .expect("an icons run");
    assert_eq!(
        run_steps(&scenario(steps), icons, &variables(icons, &placeholders())),
        Ok(vec![
            (1, json!({ "click": { "entry": "archive" }, "count": 2 })),
            (2, json!({ "wait": { "view_mode": "Icons" } })),
            (3, json!({ "key": "Escape", "when": { "dialog": true } })),
        ])
    );
    for typo in [
        json!({ "click": { "entry": "a", "nmae": "b" } }),
        json!({ "fs": { "path": "a", "exists": true, "text": "b" } }),
        json!({ "fs": { "path": "a", "write": "b" }, "timeout_ms": 0 }),
        json!({ "fs": { "path": "a", "is": "socket" } }),
        json!({ "key": "Escape", "when": { "dialgo": true } }),
        json!({ "holds": { "dialog": true } }),
        json!({ "layout": "klingon" }),
        json!({ "expect": { "dialog": true }, "as": "open", "when": { "dialog": true } }),
        json!({ "wait": { "all": [] } }),
        json!({ "fs": { "path": "a", "volume": "/dev/shm", "with": { "../b": "c" } } }),
        json!({ "fs": { "path": "a", "write": "b", "with": { "c": "d" } } }),
    ] {
        assert!(
            run_steps(
                &scenario(vec![typo.clone()]),
                icons,
                &variables(icons, &placeholders())
            )
            .is_err(),
            "{typo} should be rejected before the run starts"
        );
    }
}

#[test]
fn notes_stand_in_for_what_a_run_reads_and_unknown_variables_are_rejected() {
    let header = || -> Header {
        serde_json::from_value(json!({
            "scenario": "example", "modes": ["columns"], "twin": "x", "mutation": null,
            "known_bug": null, "variants": [{ "id": "name" }, { "id": "row-space" }],
        }))
        .expect("the header should parse")
    };
    let runs = runs(&header());
    let row_space = runs
        .iter()
        .find(|run| run.variant.id == "row-space")
        .expect("a row-space run");
    let check = |steps: Vec<Value>| {
        let scenario = Scenario {
            header: header(),
            steps: steps.into_iter().enumerate().collect(),
        };
        run_steps(&scenario, row_space, &variables(row_space, &placeholders()))
    };
    let kept = check(vec![
        json!({ "note": "entries", "dir": "$root", "as": "names" }),
        json!({ "wait": { "entries": "$names" } }),
        json!({ "note": "middle_visible", "unless_first": "a", "as": "first" }),
        json!({ "increment": "first", "as": "first_next" }),
        json!({ "fs": { "path": "archive/$first/x", "mkdir": true } }),
        json!({ "wait": { "focused": "$first_next" } }),
        json!({ "key": "Escape", "variants": ["name"] }),
    ])
    .expect("notes should stand in for their values");
    assert_eq!(
        kept.len(),
        6,
        "the name variant's step stays out of the row-space run"
    );
    assert_eq!(kept[1].1, json!({ "wait": { "entries": "$names" } }));
    for broken in [
        vec![json!({ "wait": { "focused": "$cursor" } })],
        vec![
            json!({ "wait": { "focused": "$cursor" } }),
            json!({ "note": "focused", "as": "cursor" }),
        ],
        vec![json!({ "note": "focused", "as": "root" })],
        vec![json!({ "note": "focused", "as": "cursor", "when": { "dialog": null } })],
        vec![json!({ "increment": "names", "as": "next" })],
        vec![
            json!({ "note": "entries", "as": "names" }),
            json!({ "increment": "names", "as": "next" }),
        ],
    ] {
        assert!(
            check(broken.clone()).is_err(),
            "{broken:?} should be rejected"
        );
    }

    let directory = tempfile::tempdir().expect("a scratch directory");
    let path = directory.path().join("example.jsonl");
    fs::write(
        &path,
        "{\"scenario\":\"example\",\"modes\":[\"columns\"],\"twin\":\"x\",\"mutation\":null,\
         \"variants\":[{\"id\":\"name\"}]}\n{\"key\":\"Escape\",\"variants\":[\"missing\"]}\n",
    )
    .expect("the scenario should be written");
    assert!(
        load_scenario(&path).is_err(),
        "an unknown step variant is a typo"
    );

    let notes = vec![
        ("names".to_owned(), json!(["a.txt", "b.txt"])),
        ("first".to_owned(), json!("folder-087")),
    ];
    assert_eq!(
        substitute_notes(
            &json!({ "wait": { "selected": "$names" }, "message": "to become $names in $first" }),
            &notes
        ),
        json!({
            "wait": { "selected": ["a.txt", "b.txt"] },
            "message": "to become ['a.txt', 'b.txt'] in folder-087",
        })
    );
    assert_eq!(increment("folder-087"), Ok("folder-088".to_owned()));
    assert_eq!(increment("folder-099"), Ok("folder-100".to_owned()));
    assert!(increment("readme").is_err());
}

#[test]
fn fs_actions_make_numbered_directories_and_pin_a_populated_tree() {
    let root = tempfile::tempdir().expect("a scratch directory");
    let act = |step: Value| {
        let step: FsStep = serde_json::from_value(step).expect("the step should parse");
        step.fs.observe(
            root.path(),
            SystemTime::UNIX_EPOCH + Duration::from_secs(1_000),
            &mut FsState::default(),
        )
    };
    fs::create_dir(root.path().join("archive")).expect("the directory should be created");
    act(json!({ "fs": { "path": "archive/folder-{08..10}", "mkdir": true } }))
        .expect("numbered directories should be made");
    let mut made: Vec<String> = fs::read_dir(root.path().join("archive"))
        .expect("the directory should list")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    made.sort();
    assert_eq!(made, ["folder-08", "folder-09", "folder-10"]);
    assert!(
        act(json!({ "fs": { "path": "archive/folder-09", "mkdir": true } })).is_err(),
        "Path.mkdir() refuses an existing directory"
    );
    act(json!({ "fs": { "path": ".", "populate": { "docs": { "{1..2}.txt": "n{}" } } } }))
        .expect("the tree should be populated");
    assert_eq!(
        fs::read_to_string(root.path().join("docs/2.txt")).expect("the file should exist"),
        "n2"
    );
    let pinned = |path: &str| {
        fs::metadata(root.path().join(path))
            .and_then(|metadata| metadata.modified())
            .expect("the time should be readable")
    };
    let expected = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
    assert_eq!(pinned("docs/1.txt"), expected);
    assert_eq!(
        pinned("archive/folder-08"),
        expected,
        "populate pins everything below it"
    );
}

#[test]
fn a_variable_replaces_only_its_whole_token() {
    let variables = [
        ("view".to_owned(), "Icons".to_owned()),
        ("root".to_owned(), "fixture".to_owned()),
    ];
    let replaced = substitute_text("$view, $viewport, $root/x, $$view, $", &variables);
    assert_eq!(replaced, "Icons, $viewport, fixture/x, $$view, $");
    assert_eq!(
        unescape(&json!(replaced)),
        json!("Icons, $viewport, fixture/x, $view, $"),
        "$$ is a literal $ once nothing is left to replace"
    );
    assert_eq!(unknown_variable(&json!("Energy $$E=mc^2$$")), None);
}

#[test]
fn a_known_bug_must_name_a_step_line_that_can_fail() {
    let directory = tempfile::tempdir().expect("a scratch directory");
    let path = directory.path().join("example.jsonl");
    let load = |marker: &str| {
        fs::write(
            &path,
            format!(
                "{{\"scenario\":\"example\",\"modes\":[\"columns\"],\"twin\":\"x\",\"mutation\":null{marker}}}\n\
                 {{\"layout\":\"ru\"}}\n\
                 {{\"wait\":{{\"focused\":\"a\"}}}}\n\
                 {{\"key\":\"Escape\",\"when\":{{\"dialog\":true}}}}\n\
                 {{\"flag\":{{\"dialog\":null}},\"as\":\"closed\"}}\n\
                 {{\"note\":\"focused\",\"as\":\"name\"}}\n\
                 {{\"note\":\"entries\",\"as\":\"names\"}}\n\
                 {{\"slice\":\"names\",\"pick\":0,\"as\":\"first\"}}\n"
            ),
        )
        .expect("the scenario should be written");
        load_scenario(&path).map(|_| ())
    };
    assert!(load(r#","known_bug":"issue-1","known_bug_step":3"#).is_ok());
    for marker in [
        r#","known_bug":"issue-1""#,
        r#","known_bug":null,"known_bug_step":3"#,
        r#","known_bug":"issue-1","known_bug_step":2"#,
        r#","known_bug":"issue-1","known_bug_step":4"#,
        r#","known_bug":"issue-1","known_bug_step":5"#,
        r#","known_bug":"issue-1","known_bug_step":6"#,
        r#","known_bug":"issue-1","known_bug_step":8"#,
        r#","known_bug":"issue-1","known_bug_step":19"#,
    ] {
        assert!(load(marker).is_err(), "{marker} should be rejected");
    }
}

#[test]
fn a_slice_keeps_the_middle_band_or_one_name_of_a_noted_list() {
    let rows: Vec<Value> = (0..7).map(|row| json!(format!("{row:03}.txt"))).collect();
    assert_eq!(
        slice(&rows, Some(3), None),
        Ok(json!(["003.txt", "004.txt", "005.txt"]))
    );
    assert_eq!(slice(&rows, Some(3), Some(-1)), Ok(json!("005.txt")));
    assert_eq!(slice(&rows, None, Some(0)), Ok(json!("000.txt")));
    assert!(slice(&rows, None, Some(7)).is_err());
}

#[test]
fn state_parity_may_ignore_only_distinct_compared_keys() {
    let directory = tempfile::tempdir().expect("a scratch directory");
    let path = directory.path().join("example.jsonl");
    let load_with = |mutation: &str, ignore: &str| {
        fs::write(
            &path,
            format!(
                "{{\"scenario\":\"example\",\"modes\":[\"columns\"],\"twin\":\"x\",\"mutation\":{mutation},\"known_bug\":null,\"state_parity_ignore\":{ignore}}}\n\
                 {{\"wait\":{{\"focused\":\"a\"}}}}\n"
            ),
        )
        .expect("the scenario should be written");
        load_scenario(&path).map(|_| ())
    };
    let load = |ignore: &str| load_with("null", ignore);
    assert!(load(r#"["focused","panes.focused"]"#).is_ok());
    assert!(
        load_with(r#""clipboard""#, r#"["focused"]"#).is_err(),
        "a scenario with a mutation keeps every state check"
    );
    assert!(load_with(r#""clipboard""#, "[]").is_ok());
    assert!(
        load(r#"["view_mode","directory","panes","focused","focused_pane","dialog"]"#).is_err(),
        "ignoring every compared key compares nothing"
    );
    assert!(load(r#"["view_mode","directory","panes.selected","panes.focused","focused","focused_pane","dialog"]"#).is_ok());
    for bad in [
        r#"["menu"]"#,
        r#"["panes.name"]"#,
        r#"["focused","focused"]"#,
        r#""focused""#,
    ] {
        assert!(load(bad).is_err(), "{bad} should be rejected");
    }
    let python = fs::read_to_string(Path::new(REPOSITORY).join("scripts/qa_parity.py"))
        .expect("qa_parity.py should be readable");
    for key in IGNORABLE_STATE {
        assert!(
            python.contains(&format!("\"{key}\"")),
            "qa_parity.py should accept {key} too"
        );
    }
}

#[test]
fn an_empty_check_counts_a_missing_directory_as_empty() {
    let root = tempfile::tempdir().expect("a scratch directory");
    fs::create_dir(root.path().join("full")).expect("the directory should be created");
    fs::write(root.path().join("full/a.txt"), "a").expect("the file should be written");
    let check = |path: &str, empty: bool| {
        let step: FsStep =
            serde_json::from_value(json!({ "fs": { "path": path, "empty": empty } }))
                .expect("the step should parse");
        step.fs
            .observe(root.path(), SystemTime::UNIX_EPOCH, &mut FsState::default())
    };
    assert!(check("missing", true).is_ok());
    assert!(check("missing", false).is_err());
    assert!(check("full", false).is_ok());
    assert!(check("full", true).is_err());
}

#[test]
fn only_a_controller_panic_is_filed_as_a_harness_error() {
    let mut outcome = Outcome::default();
    file_failure(&mut outcome, 3, "line 3", "timed out");
    file_failure(
        &mut outcome,
        4,
        "line 4",
        &format!("{CONTROLLER_PANIC}: boom"),
    );
    assert_eq!(
        outcome.failures,
        [(Some(3), "line 3: timed out".to_owned())]
    );
    assert_eq!(outcome.errors.len(), 1);
}

#[test]
fn remove_and_rename_act_only_inside_the_run_directories() {
    let scratch = tempfile::tempdir().expect("a scratch directory");
    let fixture = scratch.path().join("fixture");
    let outside = scratch.path().join("outside");
    fs::create_dir_all(fixture.join("documents")).expect("the fixture");
    fs::create_dir(&outside).expect("a directory outside the roots");
    std::os::unix::fs::symlink(&outside, fixture.join("away")).expect("a link outside");
    let roots = [fixture.clone()];
    assert!(contained(&fixture.join("documents"), &roots).is_ok());
    assert!(contained(&fixture.join("documents/../todo.txt"), &roots).is_ok());
    assert!(
        contained(&fixture.join("away"), &roots).is_ok(),
        "the link itself is inside"
    );
    assert!(contained(&fixture.join("away/escaped.txt"), &roots).is_err());
    assert!(contained(&fixture.join("../outside/x"), &roots).is_err());
    assert!(contained(&fixture, &[scratch.path().join("fixture/documents")]).is_err());
    assert!(contained(Path::new("/etc/passwd"), &roots).is_err());
}

#[test]
fn file_parts_join_repeats_and_fs_actions_write_archives_and_sparse_files() {
    let manifest = manifest();
    let tree = expand_tree(
        &json!({ "table.csv": ["name,value\n", { "repeat": "record-{},{}\n", "from": 0, "to": 2 }] }),
        &manifest,
    )
    .expect("parts should expand");
    assert_eq!(
        tree["table.csv"],
        json!("name,value\nrecord-0,0\nrecord-1,1\nrecord-2,2\n")
    );
    assert!(expand_tree(&json!({ "a": [{ "repeat": "x" }] }), &manifest).is_err());
    assert_eq!(stem("042.txt").as_deref(), Ok("042"));

    let root = tempfile::tempdir().expect("a scratch directory");
    let act = |step: Value| {
        let step: FsStep = serde_json::from_value(step).expect("the step should parse");
        assert!(
            step.fs.acts(),
            "an archive, a truncation, and a copy act at once"
        );
        step.fs
            .observe(root.path(), SystemTime::UNIX_EPOCH, &mut FsState::default())
    };
    act(json!({ "fs": { "path": "a.zip", "zip": [
        ["3D/3dmodel.model", "geometry"],
        ["Metadata/thumbnail.png", { "image": { "width": 2, "height": 2, "rgb": [255, 0, 0] } }],
    ] } }))
    .expect("the archive should be written");
    let mut archive =
        zip::ZipArchive::new(File::open(root.path().join("a.zip")).expect("the archive"))
            .expect("a readable zip");
    assert_eq!(
        archive.file_names().collect::<Vec<_>>(),
        ["3D/3dmodel.model", "Metadata/thumbnail.png"]
    );
    assert_eq!(
        archive.by_index(0).expect("an entry").compression(),
        zip::CompressionMethod::Stored,
        "zipfile writes stored entries by default"
    );
    act(json!({ "fs": { "path": "big.stl", "truncate": 4097 } }))
        .expect("the file should be sized");
    assert_eq!(
        fs::metadata(root.path().join("big.stl"))
            .map(|m| m.len())
            .ok(),
        Some(4097)
    );
    act(json!({ "fs": { "path": "copy.json", "from_repository": "tests/fixtures/harness.json" } }))
        .expect("the repository file should be copied");
    assert_eq!(
        fs::read(root.path().join("copy.json")).ok(),
        fs::read(Path::new(REPOSITORY).join("tests/fixtures/harness.json")).ok()
    );
    let escape: FsStep = serde_json::from_value(
        json!({ "fs": { "path": "x", "from_repository": "../etc/passwd" } }),
    )
    .expect("the step should parse");
    assert!(
        escape.fs.kind().is_err(),
        "from_repository stays inside the repository"
    );
}

#[test]
fn a_flag_keeps_whether_a_predicate_held_and_always_runs() {
    assert!(check_step(&json!({ "flag": { "dialog": null }, "as": "enabled" })).is_ok());
    assert!(check_step(&json!({ "flag": { "dialog": null } })).is_err());
    assert!(check_step(&json!({ "flag": { "nope": 1 }, "as": "enabled" })).is_err());
    assert!(
        check_step(
            &json!({ "flag": { "dialog": null }, "as": "enabled", "when": { "dialog": null } })
        )
        .is_err(),
        "a flag always runs"
    );
}

#[test]
fn input_parity_may_ignore_only_named_positions_of_one_run() {
    let directory = tempfile::tempdir().expect("a scratch directory");
    let path = directory.path().join("example.jsonl");
    let load_with = |mutation: &str, ignore: &str| {
        fs::write(
            &path,
            format!(
                "{{\"scenario\":\"example\",\"modes\":[\"columns\"],\"twin\":\"x[$v]\",\"mutation\":{mutation},\"known_bug\":null,\
                 \"variants\":[{{\"id\":\"wheel\",\"vars\":{{\"v\":\"w\"}}}},{{\"id\":\"reset\",\"vars\":{{\"v\":\"r\"}}}}],\
                 \"input_parity_ignore\":{ignore}}}\n\
                 {{\"wait\":{{\"focused\":\"a\"}}}}\n"
            ),
        )
        .expect("the scenario should be written");
        load_scenario(&path).map(|_| ())
    };
    let load = |ignore: &str| load_with("null", ignore);
    assert!(load(r#"[{"run":"columns-wheel","events":[{"position":19,"event":"button 1 down"},{"position":20,"event":"button 1 up"}]}]"#).is_ok());
    assert!(load("[]").is_ok());
    assert!(
        load_with(
            r#""quick-preview""#,
            r#"[{"run":"columns-wheel","positions":[19]}]"#
        )
        .is_err(),
        "a scenario with a mutation keeps every input check"
    );
    for bad in [
        r#"[{"run":"columns","events":[{"position":1,"event":"button 1 down"}]}]"#,
        r#"[{"run":"columns-wheel","events":[]}]"#,
        r#"[{"run":"columns-wheel","events":[{"position":0,"event":"button 1 down"}]}]"#,
        r#"[{"run":"columns-wheel","events":[{"position":2,"event":"button 1 down"},{"position":2,"event":"button 1 up"}]}]"#,
        r#"[{"run":"columns-wheel","events":[{"position":2,"event":"key 37 down"}]}]"#,
        r#"[{"run":"columns-wheel","events":[{"position":2}]}]"#,
        r#"[{"run":"columns-wheel","positions":[19]}]"#,
        r#"[{"run":"columns-wheel","events":[{"position":1,"event":"button 1 down"}]},{"run":"columns-wheel","events":[{"position":2,"event":"button 1 up"}]}]"#,
    ] {
        assert!(load(bad).is_err(), "{bad} should be rejected");
    }
    let python = fs::read_to_string(Path::new(REPOSITORY).join("scripts/qa_parity.py"))
        .expect("qa_parity.py should be readable");
    assert!(
        python.contains("def input_parity_ignore("),
        "qa_parity.py should read input_parity_ignore too"
    );
}

#[test]
fn fs_actions_write_the_twins_archives_thumbnails_and_records() {
    let root = tempfile::tempdir().expect("a scratch directory");
    let mut state = FsState {
        roots: vec![root.path().to_owned()],
        ..FsState::default()
    };
    let mut step = |value: Value| {
        let step: FsStep = serde_json::from_value(value).expect("the step should parse");
        step.fs
            .observe(root.path(), SystemTime::UNIX_EPOCH, &mut state)
    };
    fs::write(root.path().join("todo.txt"), "todo\n").expect("the fixture file");
    step(json!({ "fs": { "path": "a.tar.gz", "tar_gz": {
        "mode": "755", "mtime": 1_000_000_000, "members": [["todo.txt", "todo\n"]] } } }))
    .expect("the tarball should be written");
    assert!(
        step(json!({ "fs": { "path": "a.tar.gz", "tar_members": [["todo.txt", "todo.txt"]] } }))
            .is_ok()
    );
    assert!(
        step(json!({ "fs": { "path": "a.tar.gz", "tar_members": [["other", "todo.txt"]] } }))
            .is_err()
    );
    step(json!({ "fs": { "path": "a.zip", "zip": [["todo.txt", {
        "text": "todo\n", "date_time": [2001, 9, 9, 1, 46, 40], "mode": "100755" }]] } }))
    .expect("the zip should be written");
    assert!(
        step(json!({ "fs": { "path": "a.zip", "zip_members": [["todo.txt", "todo.txt"]] } }))
            .is_ok()
    );
    assert!(step(json!({ "fs": { "path": "a.zip", "is_zip": true } })).is_ok());
    assert!(step(json!({ "fs": { "path": "todo.txt", "is_zip": false } })).is_ok());

    let cache = root.path().join("cache").display().to_string();
    step(json!({ "fs": { "path": "todo.txt", "thumbnail": {
        "cache": cache, "width": 4, "height": 2, "rgb": [0, 0, 255], "mode": "700" } } }))
    .expect("the thumbnail should be written");
    assert!(
        step(
            json!({ "fs": { "path": "todo.txt", "cached_in": cache, "pixel": {
        "at": [0, 0], "rgb": [0, 0, 255] } } })
        )
        .is_ok()
    );
    assert!(
        step(json!({ "fs": { "path": ".", "all_cached_in": cache, "except_suffix": ".zip" } }))
            .is_err(),
        "the tarball has no thumbnail"
    );

    fs::write(
        root.path().join("record"),
        r#"{"paths": ["b", "a"], "action": "keep"}"#,
    )
    .expect("a record");
    assert!(step(json!({ "fs": { "path": "record", "json": { "action": "keep" } } })).is_ok());
    assert!(step(json!({ "fs": { "path": "record", "json": { "paths": ["a", "b"] } } })).is_err());
    assert!(
        step(json!({ "fs": { "path": "record", "json_set": { "paths": ["a", "b"] } } })).is_ok()
    );

    fs::write(
        root.path().join("action.toml"),
        "name = \"x\"\n[run]\nargs = [\"a\"]\n",
    )
    .expect("a manifest");
    assert!(
        step(
            json!({ "fs": { "path": "action.toml", "toml": { "name": "x", "run.args": ["a"] } } })
        )
        .is_ok()
    );
    assert!(
        step(json!({ "fs": { "path": "action.toml", "toml": { "run.args": ["b"] } } })).is_err()
    );

    step(json!({ "fs": { "path": "bad%E8name.txt", "write": "x", "percent_path": true } }))
        .expect("a name that is not UTF-8 should be written");
    assert!(
        root.path()
            .join(
                <std::ffi::OsString as std::os::unix::ffi::OsStringExt>::from_vec(
                    b"bad\xe8name.txt".to_vec()
                )
            )
            .is_file()
    );
    assert!(
        step(json!({ "fs": { "path": "/etc/strata-qa-outside", "hex": "00" } })).is_err(),
        "a write outside the run's directories is refused"
    );
    assert_eq!(quote_path(b"/tmp/a b/bad\xe8.txt"), "/tmp/a%20b/bad%E8.txt");
    assert_eq!(
        &raw_photo()[..4],
        b"II\x2a\0",
        "the DNG starts with a little-endian TIFF header"
    );
    assert!(
        pdf_image(&Image {
            width: 4,
            height: 2,
            rgb: [0, 0, 255],
            width_step: 0,
            thumbnail: None
        })
        .is_ok_and(|pdf| pdf.starts_with(b"%PDF-"))
    );
}

#[test]
fn log_proc_and_each_steps_check_their_shape_before_the_run() {
    for valid in [
        json!({ "log": { "count": "started" }, "as": "initial" }),
        json!({ "log": { "count": "started", "at_most": 2 }, "timeout_ms": 0 }),
        json!({ "log": { "matches": "pid=(\\d+)", "pick": -1 }, "as": "pid" }),
        json!({ "log": { "contains": "Landlock" }, "as": "landlocked" }),
        json!({ "proc": { "gone": [1, 2] } }),
        json!({ "proc": { "kill": 7 } }),
        json!({ "restart": true }),
        json!({ "dbus_send": ["--print-reply"] }),
        json!({ "each": "names", "as": "name", "step": { "wait": { "entry": "$name" } } }),
    ] {
        assert!(check_step(&valid).is_ok(), "{valid} should be accepted");
    }
    for invalid in [
        json!({ "log": { "count": "started" } }),
        json!({ "log": { "count": "a", "contains": "b" }, "as": "x" }),
        json!({ "log": { "contains": "a", "at_least": 1 } }),
        json!({ "log": { "contains": "a" }, "as": "x", "if": "y" }),
        json!({ "proc": { "kill": 7 }, "timeout_ms": 0 }),
        json!({ "restart": true, "message": "x" }),
        json!({ "dbus_send": [] }),
        json!({ "each": "names", "as": "name", "step": { "fs": { "path": "a", "exists": true } } }),
    ] {
        assert!(
            check_step(&invalid).is_err(),
            "{invalid} should be rejected"
        );
    }
    let kill: ProcStep =
        serde_json::from_value(json!({ "proc": { "kill": 1 } })).expect("a kill should parse");
    assert!(
        kill.run(std::process::id())
            .is_err_and(|error| error.starts_with("refusing to kill")),
        "a pid that is not the application's child is never signalled"
    );
    let directory = tempfile::tempdir().expect("a scratch directory");
    let path = directory.path().join("example.jsonl");
    for (name, allowed) in [
        ("PATH", true),
        ("DISPLAY", false),
        ("HOME", false),
        ("XDG_CACHE_HOME", false),
    ] {
        fs::write(
            &path,
            format!(
                "{{\"scenario\":\"example\",\"modes\":[\"columns\"],\"twin\":\"x\",\"mutation\":null,\
                 \"known_bug\":null,\"env\":{{\"{name}\":\"x\"}}}}\n{{\"key\":\"Escape\"}}\n"
            ),
        )
        .expect("the scenario should be written");
        assert_eq!(load_scenario(&path).is_ok(), allowed, "env {name}");
    }
    let each: EachStep = serde_json::from_value(json!({
        "each": "names", "as": "name", "derive": { "caption": { "number_plus": 320, "suffix": "×180" } },
        "step": { "wait": { "present": { "role": "label", "name": "$caption" } }, "message": "for $name" },
    }))
    .expect("the each step should parse");
    assert_eq!(
        each.expand(&json!("photo-0012.png")),
        Ok(
            json!({ "wait": { "present": { "role": "label", "name": "332×180" } }, "message": "for photo-0012.png" })
        )
    );
}
