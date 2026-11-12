#!/usr/bin/env bash
# Parity stack bring-up (NEWFRONT-19). One command from a clean checkout:
# builds the scratch API image, starts Postgres plus the API, waits for it,
# seeds the deterministic parity workspace, and warms the oracle's entry
# routes so the first specs never meet a cold dev server. Idempotent: safe
# to rerun; image builds reuse cached layers unless the Dockerfile or its
# copied inputs changed (pass --rebuild for a clean no-cache rebuild);
# for a from-zero data rebuild use parity-reset.sh.
#
# Namespaces (NEWFRONT-132): export PARITY_NS once (e.g. PARITY_NS=257)
# and this stack gets its own compose project, container names, ports,
# volumes and seed file, isolated from every other checkout's stack. The
# resolver (parity-env.sh, same algorithm as helpers/parity-env.ts) owns
# the mapping; explicit PARITY_* settings always win, and with no
# namespace set every value is exactly the legacy default.
#
# Editions (NEWFRONT-436): PARITY_ORACLE_EDITION=cloud serves the
# cloud-edition oracle (overlaid API plus overlaid production oracle)
# instead of the OSS one; unset means OSS, exactly as before. One stack
# serves one edition — run a second namespace for the other.
set -euo pipefail

export STACK_DIR
STACK_DIR="$(cd "$(dirname "$0")" && pwd)"
export REPO_ROOT
REPO_ROOT="$(cd "$STACK_DIR/../../../../.." && pwd)"
export PARITY_DIR
PARITY_DIR="$(cd "$STACK_DIR/.." && pwd)"
export COMPOSE_FILE
COMPOSE_FILE="$STACK_DIR/docker-compose.yml"

# Two steps on purpose: eval'ing the substitution directly would mask a
# resolver failure (eval succeeds on empty input).
export PARITY_ENV_OUT
PARITY_ENV_OUT="$("$STACK_DIR/parity-env.sh")" || exit $?
eval "$PARITY_ENV_OUT"

# Fallbacks if the resolver left anything unset (explicit PARITY_*
# settings always win): the warm-up step needs the oracle port, and the
# seed path must stay namespaced, so these only fill gaps.
export PARITY_API_PORT
PARITY_API_PORT="${PARITY_API_PORT:-18019}"
export PARITY_ORACLE_PORT
PARITY_ORACLE_PORT="${PARITY_ORACLE_PORT:-13000}"
export PARITY_SEED_FILE
PARITY_SEED_FILE="${PARITY_SEED_FILE:-$PARITY_DIR/.seed.json}"
export PARITY_ORACLE_ADMIN_URL
if [ -z "${PARITY_ORACLE_ADMIN_URL:-}" ]; then
  PARITY_ORACLE_ADMIN_URL="$(printf '%s' "${PARITY_ORACLE_URL:-http://localhost:$PARITY_ORACLE_PORT}" | sed 's:/*$::')/god-mode"
fi

# Image builds are cache-aware (NEWFRONT-134): a rerun reuses every layer
# unless the Dockerfile or its copied inputs changed, so the oracle
# container is NOT recreated by unrelated edits. Pass --rebuild (or set
# PARITY_REBUILD=1) for a clean no-cache rebuild of both images.
export PARITY_REBUILD
PARITY_REBUILD="${PARITY_REBUILD:-0}"
if [ "${1:-}" = "--rebuild" ]; then
  PARITY_REBUILD=1
elif [ -n "${1:-}" ]; then
  echo "usage: parity-up.sh [--rebuild]" >&2
  exit 2
fi

# Which edition of the old app serves as the oracle (NEWFRONT-436). oss
# (default): the open-source app and API, exactly as before. cloud: the
# cloud edition — the OSS API plus the private overlay (api-cloud) behind
# the overlaid production oracle (oracle-cloud-prod). Anything else fails
# fast here, before any Docker call, so a typo never boots half a stack.
export PARITY_ORACLE_EDITION
PARITY_ORACLE_EDITION="${PARITY_ORACLE_EDITION:-oss}"
export API_SERVICE
API_SERVICE="api"
export SEED_SCRIPT
SEED_SCRIPT="$STACK_DIR/seed/seed_parity.py"
export SEED_LABEL
SEED_LABEL="parity"
case "$PARITY_ORACLE_EDITION" in
  oss) ;;
  cloud)
    if [ "${PARITY_ORACLE_MODE:-prod}" = "dev" ]; then
      echo "[parity] refusing: PARITY_ORACLE_MODE=dev has no cloud oracle yet; unset it for the cloud production oracle" >&2
      exit 2
    fi
    # shellcheck source=lib/parity-cloud.sh
    . "$STACK_DIR/lib/parity-cloud.sh"
    export CLOUD_OVERLAY_DIR
    CLOUD_OVERLAY_DIR="$(parity_cloud_resolve_overlay)" || exit 1
    parity_cloud_stage_overlay "$CLOUD_OVERLAY_DIR" || exit 1
    API_SERVICE="api-cloud"
    SEED_SCRIPT="$STACK_DIR/seed/seed_cloud.py"
    SEED_LABEL="cloud"
    ;;
  *)
    echo "[parity] refusing: PARITY_ORACLE_EDITION must be 'oss' or 'cloud', got '$PARITY_ORACLE_EDITION'" >&2
    exit 2
    ;;
esac

echo "[parity] repo root: $REPO_ROOT"
echo "[parity] stack project: $PARITY_PROJECT (namespace '${PARITY_NS:-<none>}')"
echo "[parity] oracle edition: $PARITY_ORACLE_EDITION"
# Which build of the old app serves as the oracle. prod (default): the
# production build behind nginx, far lighter on CPU and memory (see
# oracle-prod in docker-compose.yml). dev: the dev server, as before.
# The admin oracle follows the same switch (NEWFRONT-441), so one mode
# selects both frontends and runs never mix a prod web oracle with a dev
# admin oracle.
export ORACLE_SERVICE
ORACLE_SERVICE="oracle-prod"
export ORACLE_UPSTREAM
ORACLE_UPSTREAM="oracle-prod:3000"
export ORACLE_ADMIN_SERVICE
ORACLE_ADMIN_SERVICE="oracle-admin-prod"
export ORACLE_ADMIN_UPSTREAM
ORACLE_ADMIN_UPSTREAM="oracle-admin-prod:3000"
if [ "${PARITY_ORACLE_MODE:-prod}" = "dev" ]; then
  ORACLE_SERVICE="oracle"
  ORACLE_UPSTREAM="oracle:3000"
  ORACLE_ADMIN_SERVICE="oracle-admin"
  ORACLE_ADMIN_UPSTREAM="oracle-admin:3000"
fi
# The cloud edition serves its own oracle builds; the mode knob above is
# OSS-only (a cloud+dev combination already refused further up). The
# admin oracle follows the edition too (NEWFRONT-446): the cloud admin
# console swaps the overlay home page in, so the OSS admin build would
# serve the wrong entry gate.
if [ "$PARITY_ORACLE_EDITION" = "cloud" ]; then
  ORACLE_SERVICE="oracle-cloud-prod"
  ORACLE_UPSTREAM="oracle-cloud-prod:3000"
  ORACLE_ADMIN_SERVICE="oracle-admin-cloud-prod"
  ORACLE_ADMIN_UPSTREAM="oracle-admin-cloud-prod:3000"
fi

# Preflight: this project must not already belong to another checkout,
# and the ports it publishes must be free or ours. Either finding is a
# namespace collision, and failing here beats a half-built stack or,
# worse, two stacks silently sharing a database. Reruns stay idempotent:
# this checkout's own containers exempt their ports.
echo "[parity] preflight: checking ownership and published ports for project $PARITY_PROJECT"
"$STACK_DIR/parity-guard.sh" "$PARITY_PROJECT" "$STACK_DIR" \
  --ports "$PARITY_PG_PORT,$PARITY_REDIS_PORT,$PARITY_API_PORT,$PARITY_ORACLE_PORT,$PARITY_MINIO_PORT"

echo "[parity] checking host disk before the build"
"$STACK_DIR/parity-disk-check.sh"

# api-cloud layers on the OSS api image, so the cloud path builds the base
# first; one build call would not order them. (No flag array: empty-array
# expansion trips `set -u` on the bash 3.2 macOS still ships.)
if [ "$PARITY_REBUILD" = "1" ]; then
  echo "[parity] rebuilding scratch images from zero (${PARITY_CONTAINER_PREFIX:-parity19}-$API_SERVICE, $ORACLE_SERVICE, $ORACLE_ADMIN_SERVICE)"
  if [ "$PARITY_ORACLE_EDITION" = "cloud" ]; then
    docker compose -p "$PARITY_PROJECT" -f "$COMPOSE_FILE" build --no-cache api
  fi
  docker compose -p "$PARITY_PROJECT" -f "$COMPOSE_FILE" build --no-cache "$API_SERVICE" "$ORACLE_SERVICE" "$ORACLE_ADMIN_SERVICE"
else
  echo "[parity] building scratch images (${PARITY_CONTAINER_PREFIX:-parity19}-$API_SERVICE, $ORACLE_SERVICE, $ORACLE_ADMIN_SERVICE; cached unless inputs changed)"
  if [ "$PARITY_ORACLE_EDITION" = "cloud" ]; then
    docker compose -p "$PARITY_PROJECT" -f "$COMPOSE_FILE" build api
  fi
  docker compose -p "$PARITY_PROJECT" -f "$COMPOSE_FILE" build "$API_SERVICE" "$ORACLE_SERVICE" "$ORACLE_ADMIN_SERVICE"
fi

if [ "$PARITY_ORACLE_EDITION" = "cloud" ]; then
  # --no-deps: the proxy's static depends_on names the OSS `api`, which
  # must not boot in a cloud stack (it would collide on the API port), so
  # every service here is explicit — minio included, since it otherwise
  # rides along only via that depends_on. No worker: nothing in the cloud
  # area needs background fan-out.
  echo "[parity] starting pg, redis, mq, minio, api-cloud, $ORACLE_SERVICE, $ORACLE_ADMIN_SERVICE, proxy"
  docker compose -p "$PARITY_PROJECT" -f "$COMPOSE_FILE" up -d --no-deps pg redis mq minio api-cloud "$ORACLE_SERVICE" "$ORACLE_ADMIN_SERVICE" proxy
else
  echo "[parity] starting pg, redis, mq, api, worker, $ORACLE_SERVICE, $ORACLE_ADMIN_SERVICE, proxy"
  docker compose -p "$PARITY_PROJECT" -f "$COMPOSE_FILE" up -d pg redis mq api worker "$ORACLE_SERVICE" "$ORACLE_ADMIN_SERVICE" proxy
fi

# Only one admin oracle may answer the shared `oracle-admin-upstream`
# alias (see docker-compose.yml): stop every non-selected one, so a
# rerun that switches PARITY_ORACLE_MODE or PARITY_ORACLE_EDITION never
# splits /god-mode traffic between the admin builds. Absent is fine.
# (No flag array: empty-array expansion trips `set -u` on the bash 3.2
# macOS still ships.)
for admin_service in oracle-admin oracle-admin-prod oracle-admin-cloud-prod; do
  if [ "$admin_service" != "$ORACLE_ADMIN_SERVICE" ]; then
    docker compose -p "$PARITY_PROJECT" -f "$COMPOSE_FILE" --profile oracle-prod --profile cloud stop "$admin_service" > /dev/null 2>&1 || true
  fi
done

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
  PENDING="$(docker compose -p "$PARITY_PROJECT" -f "$COMPOSE_FILE" exec -T "$API_SERVICE" python manage.py showmigrations 2>/dev/null | grep -c '\[ \]' || true)"
  if [ "$PENDING" = "0" ]; then
    break
  fi
  sleep 10
done
docker compose -p "$PARITY_PROJECT" -f "$COMPOSE_FILE" exec -T "$API_SERVICE" python manage.py migrate --check
echo "[parity] schema is current"

echo "[parity] bootstrapping the instance (idempotent)"
docker compose -p "$PARITY_PROJECT" -f "$COMPOSE_FILE" exec -T "$API_SERVICE" python manage.py register_instance parity19-scratch > /dev/null
docker compose -p "$PARITY_PROJECT" -f "$COMPOSE_FILE" exec -T "$API_SERVICE" python manage.py configure_instance > /dev/null
docker compose -p "$PARITY_PROJECT" -f "$COMPOSE_FILE" exec -T "$API_SERVICE" python manage.py shell -c "from pi_dash.license.models import Instance; Instance.objects.filter(is_setup_done=False).update(is_setup_done=True)"

echo "[parity] seeding the $SEED_LABEL workspace"
export SEED_OUT
SEED_OUT="$(docker compose -p "$PARITY_PROJECT" -f "$COMPOSE_FILE" exec -T "$API_SERVICE" python manage.py shell < "$SEED_SCRIPT")"
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

# Warm the oracle before any spec runs (NEWFRONT-134). The dev server pays
# its dependency-optimization spike on the first HTTP requests; without
# this step the suite's first specs burn their waits on a cold boot and
# flake. Entry plus one seeded project route, through the proxy — the same
# two loads the manual workaround used. A 3xx counts as served (deep routes
# redirect anonymous visitors to sign-in). On the production oracle this
# passes instantly and doubles as a smoke test. The cloud oracle warms its
# marketing surface instead: the root redirect plus the two overlay pages
# the smoke steps prove (NEWFRONT-436).
if [ "$PARITY_ORACLE_EDITION" = "cloud" ]; then
  echo "[parity] warming the oracle (entry + marketing routes)"
else
  echo "[parity] warming the oracle (entry + project routes)"
fi
export WARM_BASE
WARM_BASE="http://localhost:$PARITY_ORACLE_PORT"
export WARM_DEEP
WARM_DEEP="$(echo "$SEED_JSON" | python3 -c "import json,sys; d=json.load(sys.stdin); print('/%s/projects/%s/issues' % (d['workspaceSlug'], d['projectId']))" || echo "/")"
export WARM_ROUTES
# The admin console entry rides along (NEWFRONT-441): same proxy, the
# /god-mode mount. The admin SPA answers its entry with the shell (a 2xx
# carrying the loader, then setup or sign-in client-side), so a 2xx/3xx
# here proves the whole mount — proxy route, upstream, basename — serves.
WARM_ROUTES="/ $WARM_DEEP /god-mode/"
if [ "$PARITY_ORACLE_EDITION" = "cloud" ]; then
  # The cloud admin entry rides along too (NEWFRONT-446): the overlaid
  # home page is client-rendered, so a 2xx/3xx on the mount proves the
  # overlay-swapped admin build serves behind the proxy.
  WARM_ROUTES="/ /home /login /god-mode/"
fi
for route in $WARM_ROUTES; do
  warmed=0
  for _ in $(seq 1 120); do
    code="$(curl -s -o /dev/null -w '%{http_code}' --max-time 30 "$WARM_BASE$route" || true)"
    case "$code" in
      2*|3*) warmed=1; break ;;
    esac
    sleep 5
  done
  if [ "$warmed" != "1" ]; then
    echo "[parity] oracle did not serve $route after 10 minutes; check 'docker logs <prefix>-oracle' (on a loaded host this usually means the dev server was OOM-killed during dependency optimization)" >&2
    exit 1
  fi
  echo "[parity] warm: $route -> served"
done
echo "[parity] oracle is warm"

# The older docker-exec helpers (apiShell, desktopRuntimeShell, redis)
# default to the parity19-* container names with no namespace
# derivation, so the suite-env block must carry the resolved names
# (NEWFRONT-472). The runs* family instead reads PARITY_CONTAINER_PREFIX
# alone (runsApiContainer), so the block must carry the prefix too —
# without it, exporting just the printed block breaks namespaced runs
# with "No such container" (NEWFRONT-497). Explicit settings still win.
export PARITY_CONTAINER_PREFIX
PARITY_CONTAINER_PREFIX="${PARITY_CONTAINER_PREFIX:-${PARITY_PROJECT:-parity19}}"
export PARITY_API_CONTAINER
PARITY_API_CONTAINER="${PARITY_API_CONTAINER:-${PARITY_CONTAINER_PREFIX:-parity19}-api}"
export PARITY_REDIS_CONTAINER
PARITY_REDIS_CONTAINER="${PARITY_REDIS_CONTAINER:-${PARITY_CONTAINER_PREFIX:-parity19}-redis}"

echo "[parity] suite env for this stack (export before running specs):"
if [ -n "${PARITY_NS:-}" ]; then
  echo "export PARITY_NS='$PARITY_NS'"
fi
if [ "$PARITY_ORACLE_EDITION" = "cloud" ]; then
  echo "export PARITY_ORACLE_EDITION='cloud'"
fi
echo "export PARITY_SEED_FILE='$PARITY_SEED_FILE'"
echo "export PARITY_API_URL='$PARITY_API_URL'"
echo "export PARITY_ORACLE_URL='$PARITY_ORACLE_URL'"
echo "export PARITY_CONTAINER_PREFIX='$PARITY_CONTAINER_PREFIX'"
echo "export PARITY_API_CONTAINER='$PARITY_API_CONTAINER'"
echo "export PARITY_REDIS_CONTAINER='$PARITY_REDIS_CONTAINER'"
echo "export PARITY_ORACLE_ADMIN_URL='$PARITY_ORACLE_ADMIN_URL'"
echo "export PARITY_MINIO_PORT='$PARITY_MINIO_PORT'"
