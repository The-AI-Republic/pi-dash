#!/usr/bin/env bash
# Parity stack reset (NEWFRONT-19). Destroys the scratch database volume and
# rebuilds the stack from zero through parity-up.sh. This is the "one
# command to reset it" from the runbook.
#
# Target (always this namespace's scratch stack, never anything else): the
# compose project named by the argument, or resolved by parity-env.sh
# (default parity19) — its containers and scratch volumes. The script prints
# it before destroying anything and asserts the target project owns no
# foreign-named containers; if the names ever stop matching this stack, stop
# and ask a human instead of proceeding.
#
# Usage: parity-reset.sh [project]
# Pass the project explicitly whenever sudo is involved: sudo drops exported
# PARITY_NS / PARITY_PROJECT (env_reset), so `export PARITY_NS=...` + `sudo
# parity-reset.sh` would silently reset the default project. The script
# refuses that shape instead of guessing (NEWFRONT-174). Resetting a
# namespaced stack under sudo needs the namespace, not just the project —
# the bring-up half derives the stack's ports from it — so prefer
# `sudo env PARITY_NS=<ns> parity-reset.sh` there.
set -euo pipefail

export STACK_DIR
STACK_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=lib/parity-project.sh
. "$STACK_DIR/lib/parity-project.sh"
export COMPOSE_FILE
COMPOSE_FILE="$STACK_DIR/docker-compose.yml"

if [ "$#" -gt 1 ]; then
  echo "[parity] refusing: too many arguments (usage: parity-reset.sh [project])" >&2
  exit 1
fi

if [ -n "${1:-}" ]; then
  # Explicit argument: argv survives sudo, exported env does not.
  export PARITY_PROJECT
  PARITY_PROJECT="$(parity_resolve_project "$1")"
  export PARITY_CONTAINER_PREFIX
  PARITY_CONTAINER_PREFIX="${PARITY_CONTAINER_PREFIX:-$PARITY_PROJECT}"
else
  # No argument: resolve this checkout's stack identity (PARITY_NS-aware)
  # through the shared resolver — but refuse a guessed default under sudo.
  parity_require_identity_under_sudo "parity-reset.sh <project>  (or: sudo env PARITY_NS=<ns> parity-reset.sh)" || exit 1
  export PARITY_ENV_OUT
  PARITY_ENV_OUT="$("$STACK_DIR/parity-env.sh")" || exit $?
  eval "$PARITY_ENV_OUT"
fi

echo "[parity] reset target: compose project $PARITY_PROJECT (containers $PARITY_CONTAINER_PREFIX-*, volumes ${PARITY_PROJECT}_parity19_pgdata and ${PARITY_PROJECT}_parity19_miniodata; scratch only)"
"$STACK_DIR/parity-guard.sh" "$PARITY_PROJECT" "$STACK_DIR"
parity_assert_project "$PARITY_PROJECT"
docker compose -p "$PARITY_PROJECT" -f "$COMPOSE_FILE" down -v --remove-orphans
"$STACK_DIR/parity-up.sh"
