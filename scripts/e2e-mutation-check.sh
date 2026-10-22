#!/usr/bin/env bash
# Proves the end-to-end suite would catch a regression.
#
# Applies one deliberate defect at a time from tests/e2e/mutations, rebuilds,
# and asserts that the scenarios covering that workflow fail. The files a patch
# touches are restored afterwards.
#
#   ./scripts/e2e-mutation-check.sh              # every mutation
#   ./scripts/e2e-mutation-check.sh clipboard    # one of them
#   ./scripts/e2e-mutation-check.sh --qa [name]  # with the `--qa` scenario twins
set -euo pipefail

repository="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
mutations="$repository/tests/e2e/mutations"

# Each patch maps to the scenario that hits the mutated line, not the whole
# workflow file. Shrinking other tests in those files is fine while these fail.
declare -A SCENARIOS=(
  [drag-and-drop]="tests/e2e/scenarios/test_drag_and_drop.py::test_dragging_a_file_onto_a_folder_moves_it"
  [clipboard]="tests/e2e/scenarios/test_clipboard.py::test_copy_leaves_the_source_in_place"
  [keyboard-navigation]="tests/e2e/scenarios/test_keyboard_navigation.py::test_arrow_keys_move_focus_and_selection"
  [click-modes]="tests/e2e/scenarios/test_click_modes.py::test_single_click_opens_a_directory"
  [view-switching]="tests/e2e/scenarios/test_view_switching.py::test_appearance_menu_switches_presentation"
  [quick-preview]="tests/e2e/scenarios/test_quick_preview.py::test_space_previews_a_filtered_result_without_changing_the_query"
  [filter-results]="tests/e2e/scenarios/test_filter_results.py::test_query_updates_retain_selection_focus_preview_and_background_menu"
  [popover-scrolling]="tests/e2e/scenarios/test_popover_scrolling.py::test_panel_wheel_routing"
  [preview-scrollbar]="tests/e2e/scenarios/test_quick_preview.py::test_document_scrollbar_holds_content_still_while_rows_recycle"
  [rename-caret]="tests/e2e/scenarios/test_inline_renaming.py::test_long_rename_keeps_caret_visible"
  [click-mode-preference]="tests/e2e/scenarios/test_click_modes.py::test_changing_the_preference_takes_effect_without_restarting"
  [cut]="tests/e2e/scenarios/test_clipboard.py::test_cut_moves_only_after_paste"
  [location-entry]="tests/e2e/scenarios/test_locations.py::test_typing_a_path_navigates_there"
  [keep-both]="tests/e2e/scenarios/test_copy_conflicts.py::test_keep_both_selects_the_numbered_copy_and_undo_preserves_originals"
  [rename-enter]="tests/e2e/scenarios/test_inline_renaming.py::test_leaving_a_valid_name_commits_it[enter-existing-file-columns]"
  [rename-click-away]="tests/e2e/scenarios/test_inline_renaming.py::test_leaving_a_valid_name_commits_it[background-existing-file-columns]"
  [rename-escape]="tests/e2e/scenarios/test_inline_renaming.py::test_escape_preserves_the_original_name"
  [new-entry-collision]="tests/e2e/scenarios/test_entry_management.py::test_creating_an_existing_name_does_not_overwrite"
  [rename-error-dialog]="tests/e2e/scenarios/test_dialogs_and_menus.py::test_renaming_onto_an_existing_name_is_rejected"
  [trash]="tests/e2e/scenarios/test_entry_management.py::test_delete_moves_the_entry_to_trash"
  [shift-click-range]="tests/e2e/scenarios/test_selection.py::test_shift_click_ranges_from_the_listing_selected_entry"
)

qa=0
selected=()
for argument in "$@"; do
  if [[ "$argument" == --qa ]]; then qa=1; else selected+=("$argument"); fi
done
if ((${#selected[@]} == 0)); then
  mapfile -t selected < <(printf '%s\n' "${!SCENARIOS[@]}" | sort)
fi

if [[ -n "${STRATA_BINARY:-}" || -n "${STRATA_E2E_BUNDLE:-}" ]]; then
  echo "STRATA_BINARY and STRATA_E2E_BUNDLE must be unset so mutations exercise the rebuilt binary" >&2
  exit 1
fi

cd "$repository"
reports="target/e2e-mutations"
mkdir -p "$reports"

patched=()
restore() {
  if ((${#patched[@]})); then
    git restore --worktree -- "${patched[@]}"
  fi
  patched=()
}
trap restore EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

files_in() {
  sed -n 's|^+++ b/||p' "$1" | sort -u
}

failures=0
for name in "${selected[@]}"; do
  patch="$mutations/$name.patch"
  scenario="${SCENARIOS[$name]:-}"
  if [[ -z "$scenario" || ! -f "$patch" ]]; then
    echo "unknown mutation: $name" >&2
    exit 2
  fi
  mapfile -t files < <(files_in "$patch")
  if ((${#files[@]} == 0)); then
    echo "$name.patch names no files" >&2
    exit 2
  fi
  if [[ -n "$(git status --porcelain -- "${files[@]}")" ]]; then
    echo "$name.patch touches files with uncommitted changes; commit or stash them first" >&2
    exit 1
  fi

  twins=("$scenario")
  if ((qa)); then
    qa_scenario="$(python3 "$repository/scripts/qa_parity.py" scenario "$name")"
    echo "== $name: checking the unmodified QA scenario $qa_scenario"
    qa_baseline_status=0
    "$repository/scripts/qa.sh" "$qa_scenario" >"$reports/$name-qa-baseline.log" 2>&1 \
      || qa_baseline_status=$?
    mapfile -t twins < <(python3 "$repository/scripts/qa_parity.py" twins "$reports/$name-qa-baseline.log")
    if ((${#twins[@]} == 0)); then
      echo "   the QA scenario printed no runs; see $reports/$name-qa-baseline.log" >&2
      exit 1
    fi
  fi

  echo "== $name: checking the unmodified scenario"
  baseline_status=0
  baseline_report=()
  if ((qa)); then
    rm -f "$reports/$name-baseline.xml"
    baseline_report=(--junitxml="$reports/$name-baseline.xml")
  fi
  "$repository/scripts/e2e.sh" -q "${baseline_report[@]}" "${twins[@]}" \
    >"$reports/$name-baseline.log" 2>&1 || baseline_status=$?
  if ((qa)); then
    if ! python3 "$repository/scripts/qa_parity.py" verdict baseline \
      "$reports/$name-qa-baseline.log" "$reports/$name-baseline.xml" "$qa_baseline_status"; then
      echo "   baseline failed; see $reports/$name-qa-baseline.log and $reports/$name-baseline.log" >&2
      exit 1
    fi
  elif ((baseline_status)); then
    echo "   baseline failed; see $reports/$name-baseline.log" >&2
    exit 1
  fi

  echo "== $name: applying $patch"
  # Set first, so an interrupted or partial apply is still restored.
  patched=("${files[@]}")
  git -C "$repository" apply -p1 "$patch"
  if ((qa)); then
    echo "== $name: expecting $qa_scenario and its twins to fail together"
    qa_status=0
    "$repository/scripts/qa.sh" "$qa_scenario" >"$reports/$name-qa.log" 2>&1 || qa_status=$?
  else
    echo "== $name: expecting $scenario to fail"
  fi
  report="$reports/$name.xml"
  rm -f "$report"
  result=0
  # xdist's fail-fast shutdown exits as an interruption, not a test failure.
  # Finish the selected scenarios so detection still requires exit status 1.
  "$repository/scripts/e2e.sh" -q --maxfail=0 --junitxml="$report" "${twins[@]}" \
    >"$reports/$name.log" 2>&1 || result=$?
  if ((qa)); then
    if python3 "$repository/scripts/qa_parity.py" verdict mutated "$reports/$name-qa.log" "$report" "$qa_status"; then
      echo "   detected by both"
    else
      echo "   NOT DETECTED TOGETHER; see $reports/$name-qa.log and $reports/$name.log" >&2
      failures=$((failures + 1))
    fi
  elif python3 "$repository/scripts/e2e_mutation_result.py" "$report" "$result"; then
    echo "   detected"
  else
    echo "   NOT DETECTED or infrastructure failure; see $reports/$name.log" >&2
    failures=$((failures + 1))
  fi
  restore
done

if ((failures)); then
  echo "$failures mutation(s) went undetected" >&2
  exit 1
fi
echo "every mutation was detected"
