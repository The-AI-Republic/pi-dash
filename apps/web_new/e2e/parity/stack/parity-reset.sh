#!/usr/bin/env bash
# Parity stack reset (NEWFRONT-19). Destroys the scratch database volume and
# rebuilds the stack from zero through parity-up.sh. This is the "one
# command to reset it" from the runbook.
#
# Target (always this scratch stack, never anything else): the compose
# project named by the argument, or PARITY_PROJECT (default parity19) — its
# containers and scratch volumes. A pre-down assertion refuses when the
# project owns foreign-named containers; if it ever trips, stop and ask a
# human instead of overriding blindly.
#
# Usage: parity-reset.sh [project]
# Pass the project explicitly whenever sudo is involved: sudo drops exported
# PARITY_PROJECT (env_reset), so `export PARITY_PROJECT=...` + `sudo
# parity-reset.sh` would silently reset the default project. The script
# refuses that shape instead of guessing (NEWFRONT-174).
set -euo pipefail

export STACK_DIR
STACK_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=lib/parity-project.sh
. "$STACK_DIR/lib/parity-project.sh"
export COMPOSE_FILE
COMPOSE_FILE="$STACK_DIR/docker-compose.yml"
export PARITY_PROJECT
PARITY_PROJECT="$(parity_resolve_project "${1:-}")"

echo "[parity] reset target: compose project $PARITY_PROJECT (containers and scratch volumes only)"
parity_assert_project "$PARITY_PROJECT"
docker compose -p "$PARITY_PROJECT" -f "$COMPOSE_FILE" down -v
"$STACK_DIR/parity-up.sh"
