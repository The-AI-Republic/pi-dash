#!/usr/bin/env bash
# Parity stack teardown. Removes this stack's containers, network and scratch
# volumes. Honors PARITY_NS (via parity-env.sh) like the other scripts;
# images are kept, so the next parity-up.sh is fast.
#
# Target (always this namespace's scratch stack, never anything else): the
# compose project named by the argument, or resolved by parity-env.sh
# (default parity19).
#
# Usage: parity-down.sh [project]
# Pass the project explicitly whenever sudo is involved: sudo drops exported
# PARITY_NS / PARITY_PROJECT (env_reset), so `export PARITY_NS=...` + `sudo
# parity-down.sh` would silently target the default project. The script
# refuses that shape instead of guessing (NEWFRONT-174).
set -euo pipefail

export STACK_DIR
STACK_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=lib/parity-project.sh
. "$STACK_DIR/lib/parity-project.sh"

if [ "$#" -gt 1 ]; then
  echo "[parity] refusing: too many arguments (usage: parity-down.sh [project])" >&2
  exit 1
fi

if [ -n "${1:-}" ]; then
  # Explicit argument: argv survives sudo, exported env does not.
  export PARITY_PROJECT
  PARITY_PROJECT="$(parity_resolve_project "$1")"
else
  # No argument: resolve this checkout's stack identity (PARITY_NS-aware)
  # through the shared resolver — but refuse a guessed default under sudo.
  parity_require_identity_under_sudo "parity-down.sh <project>  (or: sudo env PARITY_NS=<ns> parity-down.sh)" || exit 1
  export PARITY_ENV_OUT
  PARITY_ENV_OUT="$("$STACK_DIR/parity-env.sh")" || exit $?
  eval "$PARITY_ENV_OUT"
fi

echo "[parity] teardown target: compose project $PARITY_PROJECT (scratch only)"
# Warn, don't refuse: teardown is also the deliberate override (and the
# reaper path), so it must work even when the owning checkout is gone.
"$STACK_DIR/parity-guard.sh" "$PARITY_PROJECT" "$STACK_DIR" --warn
parity_assert_project "$PARITY_PROJECT"
# By project name, not by file: works even after the checkout is gone.
docker compose -p "$PARITY_PROJECT" down -v --remove-orphans
# Drop the cloud overlay staging (NEWFRONT-436): proprietary files must not
# linger in the tree past the stack that used them. Best-effort — plain OSS
# stacks never create it, and teardown must succeed either way.
if [ -f "$STACK_DIR/lib/parity-cloud.sh" ]; then
  # shellcheck source=lib/parity-cloud.sh
  . "$STACK_DIR/lib/parity-cloud.sh"
  parity_cloud_clean_staging || true
fi
