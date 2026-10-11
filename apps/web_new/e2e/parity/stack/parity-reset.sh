#!/usr/bin/env bash
# Parity stack reset (NEWFRONT-19). Destroys the scratch database volume and
# rebuilds the stack from zero through parity-up.sh. This is the "one
# command to reset it" from the runbook.
#
# Target (always this namespace's scratch stack, never anything else): the
# compose project parity-env.sh resolves (default parity19). The script
# prints it before destroying anything; if the names ever stop matching
# this stack, stop and ask a human. Honors PARITY_NS like the other
# scripts, so resetting one checkout's stack never touches a sibling's.
set -euo pipefail

export STACK_DIR
STACK_DIR="$(cd "$(dirname "$0")" && pwd)"
export COMPOSE_FILE
COMPOSE_FILE="$STACK_DIR/docker-compose.yml"

export PARITY_ENV_OUT
PARITY_ENV_OUT="$("$STACK_DIR/parity-env.sh")" || exit $?
eval "$PARITY_ENV_OUT"

echo "[parity] reset target: compose project $PARITY_PROJECT (containers $PARITY_CONTAINER_PREFIX-*, volumes ${PARITY_PROJECT}_parity19_pgdata and ${PARITY_PROJECT}_parity19_miniodata; scratch only)"
"$STACK_DIR/parity-guard.sh" "$PARITY_PROJECT" "$STACK_DIR"
docker compose -p "$PARITY_PROJECT" -f "$COMPOSE_FILE" down -v --remove-orphans
"$STACK_DIR/parity-up.sh"
