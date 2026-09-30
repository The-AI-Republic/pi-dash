#!/usr/bin/env bash
# Parity stack reset (NEWFRONT-19). Destroys the scratch database volume and
# rebuilds the stack from zero through parity-up.sh. This is the "one
# command to reset it" from the runbook.
#
# Target (always this scratch stack, never anything else):
#   containers parity19-pg / parity19-redis / parity19-mq / parity19-api
#   volume     parity19_pgdata
# If those names ever stop matching this stack, stop and ask a human.
set -euo pipefail

export STACK_DIR
STACK_DIR="$(cd "$(dirname "$0")" && pwd)"
export COMPOSE_FILE
COMPOSE_FILE="$STACK_DIR/docker-compose.yml"

echo "[parity] reset target: containers parity19-* and volume parity19_pgdata (scratch only)"
docker compose -f "$COMPOSE_FILE" down -v
"$STACK_DIR/parity-up.sh"
