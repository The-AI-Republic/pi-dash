#!/usr/bin/env bash
# Parity stack bring-up (NEWFRONT-19). One command from a clean checkout:
# builds the scratch API image, starts Postgres plus the API, waits for it,
# and seeds the deterministic parity workspace. Idempotent: safe to rerun;
# for a from-zero rebuild use parity-reset.sh.
#
# Namespaces (NEWFRONT-132): export PARITY_NS once (e.g. PARITY_NS=257)
# and this stack gets its own compose project, container names, ports,
# volumes and seed file, isolated from every other checkout's stack. The
# resolver (parity-env.sh, same algorithm as helpers/parity-env.ts) owns
# the mapping; explicit PARITY_* settings always win, and with no
# namespace set every value is exactly the legacy default.
set -euo pipefail

export STACK_DIR
STACK_DIR="$(cd "$(dirname "$0")" && pwd)"
export REPO_ROOT
REPO_ROOT="$(cd "$STACK_DIR/../../../.." && pwd)"
export PARITY_DIR
PARITY_DIR="$(cd "$STACK_DIR/.." && pwd)"
export COMPOSE_FILE
COMPOSE_FILE="$STACK_DIR/docker-compose.yml"

# Two steps on purpose: eval'ing the substitution directly would mask a
# resolver failure (eval succeeds on empty input).
export PARITY_ENV_OUT
PARITY_ENV_OUT="$("$STACK_DIR/parity-env.sh")" || exit $?
eval "$PARITY_ENV_OUT"

echo "[parity] repo root: $REPO_ROOT"
echo "[parity] stack project: $PARITY_PROJECT (namespace '${PARITY_NS:-<none>}')"
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

# Preflight: this project must not already belong to another checkout,
# and the ports it publishes must be free or ours. Either finding is a
# namespace collision, and failing here beats a half-built stack or,
# worse, two stacks silently sharing a database. Reruns stay idempotent:
# this checkout's own containers exempt their ports.
echo "[parity] preflight: checking ownership and published ports for project $PARITY_PROJECT"
"$STACK_DIR/parity-guard.sh" "$PARITY_PROJECT" "$STACK_DIR" \
  --ports "$PARITY_PG_PORT,$PARITY_REDIS_PORT,$PARITY_API_PORT,$PARITY_ORACLE_PORT,$PARITY_MINIO_PORT"

echo "[parity] building scratch images (parity19-api, $ORACLE_SERVICE)"
docker compose -p "$PARITY_PROJECT" -f "$COMPOSE_FILE" build api "$ORACLE_SERVICE"

echo "[parity] starting pg, redis, mq, api, worker, $ORACLE_SERVICE, proxy"
docker compose -p "$PARITY_PROJECT" -f "$COMPOSE_FILE" up -d pg redis mq api worker "$ORACLE_SERVICE" proxy

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
  PENDING="$(docker compose -p "$PARITY_PROJECT" -f "$COMPOSE_FILE" exec -T api python manage.py showmigrations 2>/dev/null | grep -c '\[ \]' || true)"
  if [ "$PENDING" = "0" ]; then
    break
  fi
  sleep 10
done
docker compose -p "$PARITY_PROJECT" -f "$COMPOSE_FILE" exec -T api python manage.py migrate --check
echo "[parity] schema is current"

echo "[parity] bootstrapping the instance (idempotent)"
docker compose -p "$PARITY_PROJECT" -f "$COMPOSE_FILE" exec -T api python manage.py register_instance parity19-scratch > /dev/null
docker compose -p "$PARITY_PROJECT" -f "$COMPOSE_FILE" exec -T api python manage.py configure_instance > /dev/null
docker compose -p "$PARITY_PROJECT" -f "$COMPOSE_FILE" exec -T api python manage.py shell -c "from pi_dash.license.models import Instance; Instance.objects.filter(is_setup_done=False).update(is_setup_done=True)"

echo "[parity] seeding the parity workspace"
export SEED_OUT
SEED_OUT="$(docker compose -p "$PARITY_PROJECT" -f "$COMPOSE_FILE" exec -T api python manage.py shell < "$STACK_DIR/seed/seed_parity.py")"
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

echo "[parity] suite env for this stack (export before running specs):"
if [ -n "${PARITY_NS:-}" ]; then
  echo "export PARITY_NS='$PARITY_NS'"
fi
echo "export PARITY_SEED_FILE='$PARITY_SEED_FILE'"
echo "export PARITY_API_URL='$PARITY_API_URL'"
echo "export PARITY_ORACLE_URL='$PARITY_ORACLE_URL'"
