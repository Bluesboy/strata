#!/usr/bin/env bash
# Runs the `--qa` scenarios in tests/qa/scenarios/<twin test file>/ inside the pinned E2E image.
#   ./scripts/qa.sh                                  every scenario, every mode
#   ./scripts/qa.sh arrow-keys-move-focus            one scenario (hyphens or underscores)
#   ./scripts/qa.sh arrow-keys-move-focus/columns    one run of it
set -euo pipefail

unset DISPLAY WAYLAND_DISPLAY NOTIFY_SOCKET
repository="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
engine="${STRATA_CONTAINER_ENGINE:-}"
if [[ -z "$engine" ]]; then
  if command -v podman >/dev/null 2>&1; then engine=podman; else engine=docker; fi
fi
if ! command -v "$engine" >/dev/null 2>&1; then
  echo "QA scenarios require Docker or Podman; see docs/e2e-testing.md" >&2
  exit 1
fi

# CI points the Cargo home and target directory at the restored quality cache,
# which `--all-features` built in the same image ("QA in CI", docs/e2e-testing.md).
cargo_cache="$(realpath -m --relative-to="$repository" "${STRATA_E2E_CARGO_CACHE:-$repository/target/e2e-container}")"
if [[ "$cargo_cache" != target/* ]]; then
  echo "STRATA_E2E_CARGO_CACHE must name a directory under the checkout's target/." >&2
  exit 1
fi

image="${STRATA_E2E_IMAGE:-}"
if [[ -z "$image" ]]; then
  image="$(python3 "$repository/scripts/e2e_base.py" ensure --engine "$engine")"
else
  image="$(python3 "$repository/scripts/e2e_base.py" verify "$image" --engine "$engine")"
fi

user_id="$(id -u)"
group_id="$(id -g)"
accounts="$repository/target/e2e-container/accounts-$user_id-$group_id"
mkdir -p "$accounts"
printf 'root:x:0:0:root:/root:/bin/sh\n' > "$accounts/passwd"
printf 'root:x:0:\n' > "$accounts/group"
if [[ "$user_id" != 0 ]]; then
  printf 'strata-e2e:x:%s:%s:E2E:/tmp/strata-build-home:/bin/sh\n' "$user_id" "$group_id" >> "$accounts/passwd"
fi
if [[ "$group_id" != 0 ]]; then
  printf 'strata-e2e:x:%s:\n' "$group_id" >> "$accounts/group"
fi

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

options=(--rm --platform=linux/amd64 --user "$user_id:$group_id" --shm-size=512m)
if [[ "$(basename "$engine")" == podman ]]; then
  options+=(--userns=keep-id --passwd=false --security-opt 'unmask=/proc/*')
else
  options+=(--security-opt systempaths=unconfined --security-opt seccomp=unconfined --security-opt apparmor=unconfined)
fi
exec "$engine" run "${options[@]}" \
  --mount "type=bind,source=$repository,target=/workspace" \
  --mount "type=bind,source=$accounts/passwd,target=/etc/passwd,readonly" \
  --mount "type=bind,source=$accounts/group,target=/etc/group,readonly" \
  --workdir /workspace \
  --env HOME=/tmp/strata-build-home \
  --env "CARGO_HOME=/workspace/$cargo_cache/cargo" \
  --env "CARGO_TARGET_DIR=/workspace/$cargo_cache/build" \
  --env CARGO_PROFILE_DEV_DEBUG=0 --env CARGO_INCREMENTAL=0 \
  --env STRATA_QA_CONTAINER=1 \
  --env "STRATA_QA_KEEP_ARTIFACTS=${STRATA_QA_KEEP_ARTIFACTS:-0}" \
  --env "STRATA_PARITY_FIXTURES=${STRATA_PARITY_FIXTURES:-0}" \
  --env "STRATA_QA_NAMED=${named:-0}" \
  "$image" bash -euc '
    mkdir -p "$HOME" "$CARGO_HOME"
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
