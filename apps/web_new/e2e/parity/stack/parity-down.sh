#!/usr/bin/env bash
# Parity stack teardown. Removes this stack's containers, network and scratch
# volumes. Honors PARITY_NS (via parity-env.sh) like the other scripts;
# images are kept, so the next parity-up.sh is fast.
#
# Target (always this namespace's scratch stack, never anything else): the
# compose project parity-env.sh resolves (default parity19).
set -euo pipefail

export STACK_DIR
STACK_DIR="$(cd "$(dirname "$0")" && pwd)"

export PARITY_ENV_OUT
PARITY_ENV_OUT="$("$STACK_DIR/parity-env.sh")" || exit $?
eval "$PARITY_ENV_OUT"

echo "[parity] teardown target: compose project $PARITY_PROJECT (scratch only)"
# Warn, don't refuse: teardown is also the deliberate override (and the
# reaper path), so it must work even when the owning checkout is gone.
"$STACK_DIR/parity-guard.sh" "$PARITY_PROJECT" "$STACK_DIR" --warn
# By project name, not by file: works even after the checkout is gone.
docker compose -p "$PARITY_PROJECT" down -v --remove-orphans
