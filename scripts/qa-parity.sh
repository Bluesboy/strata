#!/usr/bin/env bash
# Proves each `--qa` scenario and its Python twin agree: both pass unmodified,
# both fail under the scenario's mutation at the same step, and both send the
# same input and end in the same window state. Runs only the pinned container
# runners (scripts/qa.sh, scripts/e2e.sh), one after the other; reports go to
# target/qa-parity. Exits non-zero when any pair disagrees.
#
#   ./scripts/qa-parity.sh              every scenario pair
#   ./scripts/qa-parity.sh clipboard    the pair a mutation belongs to
set -euo pipefail

repository="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
exec python3 "$repository/scripts/qa_parity.py" run "$@"
