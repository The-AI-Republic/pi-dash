#!/usr/bin/env bash
# Parity stack teardown. Removes this stack's containers, network and scratch
# volumes. Honors PARITY_PROJECT like the other scripts; images are kept, so
# the next parity-up.sh is fast.
#
# Target (always this scratch stack, never anything else): the compose
# project named by the argument, or PARITY_PROJECT (default parity19).
#
# Usage: parity-down.sh [project]
# Pass the project explicitly whenever sudo is involved: sudo drops exported
# PARITY_PROJECT (env_reset), so `export PARITY_PROJECT=...` + `sudo
# parity-down.sh` would silently target the default project. The script
# refuses that shape instead of guessing (NEWFRONT-174).
set -euo pipefail

export STACK_DIR
STACK_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=lib/parity-project.sh
. "$STACK_DIR/lib/parity-project.sh"

export PARITY_PROJECT
PARITY_PROJECT="$(parity_resolve_project "${1:-}")"

echo "[parity] teardown target: compose project $PARITY_PROJECT (scratch only)"
parity_assert_project "$PARITY_PROJECT"
# By project name, not by file: works even after the checkout is gone.
docker compose -p "$PARITY_PROJECT" down -v --remove-orphans
