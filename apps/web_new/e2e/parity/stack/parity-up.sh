#!/usr/bin/env bash
# Parity stack bring-up (NEWFRONT-19). One command from a clean checkout:
# builds the scratch API image, starts Postgres plus the API, waits for it,
# and seeds the deterministic parity workspace. Idempotent: safe to rerun;
# for a from-zero rebuild use parity-reset.sh.
set -euo pipefail

export STACK_DIR
STACK_DIR="$(cd "$(dirname "$0")" && pwd)"
export REPO_ROOT
REPO_ROOT="$(cd "$STACK_DIR/../../../.." && pwd)"
export PARITY_DIR
PARITY_DIR="$(cd "$STACK_DIR/.." && pwd)"
export COMPOSE_FILE
COMPOSE_FILE="$STACK_DIR/docker-compose.yml"
export PARITY_API_PORT
PARITY_API_PORT="${PARITY_API_PORT:-18019}"
export PARITY_SEED_FILE
PARITY_SEED_FILE="$PARITY_DIR/.seed.json"

echo "[parity] repo root: $REPO_ROOT"
# Which build of the old app serves as the oracle. dev (default): the dev
# server, as before. prod: the production build behind nginx, far lighter on
# CPU and memory (see oracle-prod in docker-compose.yml).
export ORACLE_SERVICE
ORACLE_SERVICE="oracle"
if [ "${PARITY_ORACLE_MODE:-dev}" = "prod" ]; then
  ORACLE_SERVICE="oracle-prod"
  export ORACLE_UPSTREAM
  ORACLE_UPSTREAM="oracle-prod:3000"
fi

echo "[parity] building scratch images (parity19-api, $ORACLE_SERVICE)"
docker compose -f "$COMPOSE_FILE" build api "$ORACLE_SERVICE"

echo "[parity] starting pg, redis, mq, api, worker, $ORACLE_SERVICE, proxy"
docker compose -f "$COMPOSE_FILE" up -d pg redis mq api worker "$ORACLE_SERVICE" proxy

# Inside a Pi Dash agent run the stack is torn down when the run ends (no-op
# for a person's shell or CI; PARITY_KEEP_STACK=1 opts out).
"$STACK_DIR/parity-reaper.sh" arm || true

echo "[parity] waiting for the API on port $PARITY_API_PORT"
for _ in $(seq 1 60); do
  if python3 -c "import socket; socket.create_connection(('localhost', $PARITY_API_PORT), timeout=2).close()" 2>/dev/null; then
    break
  fi
  sleep 5
done
python3 -c "import socket; socket.create_connection(('localhost', $PARITY_API_PORT), timeout=5).close()"
echo "[parity] API is reachable"

echo "[parity] waiting for migrations to finish"
for _ in $(seq 1 120); do
  export PENDING
  PENDING="$(docker compose -f "$COMPOSE_FILE" exec -T api python manage.py showmigrations 2>/dev/null | grep -c '\[ \]' || true)"
  if [ "$PENDING" = "0" ]; then
    break
  fi
  sleep 10
done
docker compose -f "$COMPOSE_FILE" exec -T api python manage.py migrate --check
echo "[parity] schema is current"

echo "[parity] bootstrapping the instance (idempotent)"
docker compose -f "$COMPOSE_FILE" exec -T api python manage.py register_instance parity19-scratch > /dev/null
docker compose -f "$COMPOSE_FILE" exec -T api python manage.py configure_instance > /dev/null
docker compose -f "$COMPOSE_FILE" exec -T api python manage.py shell -c "from pi_dash.license.models import Instance; Instance.objects.filter(is_setup_done=False).update(is_setup_done=True)"

echo "[parity] seeding the parity workspace"
export SEED_OUT
SEED_OUT="$(docker compose -f "$COMPOSE_FILE" exec -T api python manage.py shell < "$STACK_DIR/seed/seed_parity.py")"
echo "$SEED_OUT" | grep -E "^(PARITY_SEED_JSON:|Traceback|.*Error)" || true
export SEED_JSON
SEED_JSON="$(echo "$SEED_OUT" | grep '^PARITY_SEED_JSON:' | sed 's/^PARITY_SEED_JSON://' | tail -n 1)"
if [ -z "$SEED_JSON" ]; then
  echo "[parity] seed produced no facts; full output above" >&2
  exit 1
fi
echo "$SEED_JSON" > "$PARITY_SEED_FILE"
echo "[parity] seed facts: $PARITY_SEED_FILE"
echo "$SEED_JSON" | python3 -c "import json,sys; d=json.load(sys.stdin); print('[parity] workspace', d['workspaceSlug'], '| project', d['projectId'], '| issues', len(d['issueNames']))"
