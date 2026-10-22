#!/usr/bin/env bash
# Checks each `--qa` scenario and its Python twin agree, unmodified and under the
# scenario's mutation; reports go to target/qa-parity.
#
#   ./scripts/qa-parity.sh                       every scenario pair
#   ./scripts/qa-parity.sh clipboard             the pair a mutation belongs to
#   ./scripts/qa-parity.sh two-clicks-open       a scenario pair by its name
#   ./scripts/qa-parity.sh --baseline [name...]  unmodified checks only
set -euo pipefail

repository="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
exec python3 "$repository/scripts/qa_parity.py" run "$@"
