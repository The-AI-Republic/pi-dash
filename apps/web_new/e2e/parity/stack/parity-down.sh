#!/usr/bin/env bash
# Parity stack teardown. Removes this stack's containers, network and scratch
# volumes. Honors PARITY_PROJECT like the other scripts; images are kept, so
# the next parity-up.sh is fast.
#
# Target (always this scratch stack, never anything else): the compose
# project named by PARITY_PROJECT (default parity19).
set -euo pipefail

export PARITY_PROJECT
PARITY_PROJECT="${PARITY_PROJECT:-parity19}"

echo "[parity] teardown target: compose project $PARITY_PROJECT (scratch only)"
# By project name, not by file: works even after the checkout is gone.
docker compose -p "$PARITY_PROJECT" down -v --remove-orphans
