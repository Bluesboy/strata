#!/usr/bin/env bash
# Runs the `--qa` scenarios in tests/qa/scenarios/<folder>/ inside the pinned E2E image.
#   ./scripts/qa.sh                                  every scenario, every mode
#   ./scripts/qa.sh arrow-keys-move-focus            one scenario (hyphens or underscores)
#   ./scripts/qa.sh arrow-keys-move-focus/columns    one run of it
set -euo pipefail

unset DISPLAY WAYLAND_DISPLAY NOTIFY_SOCKET
repository="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=lib/qa-container.sh
source "$repository/scripts/lib/qa-container.sh"
qa_container_prepare

# Each run is a test named `<scenario>/<run>` (tests/qa/harness.rs). A scenario
# name (hyphens or underscores) selects all of its runs, `<scenario>/<run>` one;
# flags pass to the runner's libtest-style harness unchanged, except that a
# --skip naming a scenario or one of its runs, in either spelling, becomes
# --skip-exact: it leaves out that scenario, never another it prefixes.
scenarios="$repository/tests/qa/scenarios"
arguments=()
skip() {
  local scenario="${1%%/*}"
  scenario="${scenario//_/-}"
  if [[ -n "$scenario" ]] && compgen -G "$scenarios/*/$scenario.jsonl" >/dev/null; then
    arguments+=(--skip-exact "$scenario${1#"${1%%/*}"}")
  else
    arguments+=(--skip "$1")
  fi
}
while (($#)); do
  case "$1" in
    --skip)
      skip "${2:?$1 needs a value}"
      shift 2
      ;;
    --skip=*)
      skip "${1#--skip=}"
      shift
      ;;
    --test-threads|--format|--color|-Z)
      arguments+=("$1" "${2:?$1 needs a value}")
      shift 2
      ;;
    -*)
      arguments+=("$1")
      shift
      ;;
    *)
      scenario="${1%%/*}"
      scenario="${scenario//_/-}"
      if ! compgen -G "$scenarios/*/$scenario.jsonl" >/dev/null; then
        echo "No QA scenario named ${1%%/*} in tests/qa/scenarios:" >&2
        basename -a -s .jsonl "$scenarios"/*/*.jsonl | sort >&2
        exit 2
      fi
      arguments+=("$scenario${1#"${1%%/*}"}")
      named=1
      shift
      ;;
  esac
done
# A name selects only its own scenario, never every one it prefixes; --exact
# makes any other --skip match whole names too. A named scenario runs even when its
# header's `skip` keeps it out of the default run (STRATA_QA_NAMED).
if ((${named:-0})); then
  arguments+=(--exact)
fi

qa_container_options --rm
qa_container_mounts
exec "$engine" run "${options[@]}" "${mounts[@]}" \
  --env "STRATA_QA_KEEP_ARTIFACTS=${STRATA_QA_KEEP_ARTIFACTS:-0}" \
  --env "STRATA_PARITY_FIXTURES=${STRATA_PARITY_FIXTURES:-0}" \
  --env "STRATA_QA_NAMED=${named:-0}" \
  "$image" bash -euc '
    mkdir -p "$HOME" "$CARGO_HOME"
    # The tmpfs hides the image'"'"'s /tmp, so its X socket directory is made again.
    # It belongs to this user, so xvfb.log warns "_XSERVTransmkdir: Owner of
    # /tmp/.X11-unix should be set to root"; that is harmless here.
    mkdir -p -m 1777 /tmp/.X11-unix
    rustc --version
    output="$(mktemp)"
    # Without pipefail the pipeline reports tee; the test status is kept apart.
    # Each passing run prints a PASS or KNOWN BUG line, which the check below
    # counts; the unit tests of the runner alone do not count as a QA run.
    cargo test --locked --features qa --test qa -- --include-ignored "$@" 2>&1 | tee "$output"
    status="${PIPESTATUS[0]}"
    if ((status != 0)); then exit "$status"; fi
    if ! grep -Eq "^(PASS|KNOWN BUG) " "$output"; then
      echo "The selection ran no QA scenario." >&2
      exit 1
    fi
  ' bash "${arguments[@]}"
