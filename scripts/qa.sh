#!/usr/bin/env bash
# Runs the `--qa` scenarios in tests/qa/scenarios inside the pinned E2E image.
#   ./scripts/qa.sh                          every scenario, every mode
#   ./scripts/qa.sh arrow-keys-move-focus    one scenario (hyphens or underscores)
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

# Scenario names select tests (hyphens or underscores); flags pass to libtest unchanged.
scenarios="$repository/tests/qa/scenarios"
arguments=()
while (($#)); do
  case "$1" in
    --skip|--test-threads|--format|--color|-Z)
      arguments+=("$1" "${2:?$1 needs a value}")
      shift 2
      ;;
    -*)
      arguments+=("$1")
      shift
      ;;
    *)
      name="${1//_/-}"
      if [[ ! -f "$scenarios/$name.jsonl" ]]; then
        echo "No QA scenario named $1 in tests/qa/scenarios:" >&2
        basename -a -s .jsonl "$scenarios"/*.jsonl >&2
        exit 2
      fi
      arguments+=("${name//-/_}")
      shift
      ;;
  esac
done

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
  --env CARGO_HOME=/workspace/target/e2e-container/cargo \
  --env CARGO_TARGET_DIR=/workspace/target/e2e-container/build \
  --env CARGO_PROFILE_DEV_DEBUG=0 --env CARGO_INCREMENTAL=0 \
  --env STRATA_QA_CONTAINER=1 \
  --env "STRATA_QA_KEEP_ARTIFACTS=${STRATA_QA_KEEP_ARTIFACTS:-0}" \
  "$image" bash -euc '
    mkdir -p "$HOME" "$CARGO_HOME"
    rustc --version
    output="$(mktemp)"
    # Without pipefail the pipeline reports tee; the test status is kept apart.
    # --show-output prints each passing scenario run, which the check below counts;
    # the always-on registration test alone does not count as a QA run.
    cargo test --locked --features qa --test qa -- --include-ignored --show-output "$@" 2>&1 | tee "$output"
    status="${PIPESTATUS[0]}"
    if ((status != 0)); then exit "$status"; fi
    if ! grep -Eq "^(PASS|KNOWN BUG) " "$output"; then
      echo "The selection ran no QA scenario." >&2
      exit 1
    fi
  ' bash "${arguments[@]}"
