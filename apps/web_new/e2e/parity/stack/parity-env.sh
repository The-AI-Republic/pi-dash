#!/usr/bin/env bash
# Parity-stack namespace resolver (NEWFRONT-132). Prints the resolved stack
# identity as POSIX-shell `export KEY='value'` lines on stdout, so every
# entry point agrees on what "this stack" means:
#
#   PARITY_ENV_OUT="$(stack/parity-env.sh)" || exit $?
#   eval "$PARITY_ENV_OUT"
#
# (Two steps: eval'ing the substitution directly would mask a resolver
# failure, since eval succeeds on empty input.)
#
# PARITY_NS (one setting, e.g. 257) derives the compose project
# (parity257), the container prefix, the six published ports (per-service
# base plus the namespace's last two digits: 13057, 13157, 15457, 16357,
# 18057, 19057), the seed-file path and the suite URLs. Explicit PARITY_*
# settings always win; with no namespace set every value is exactly the
# legacy default. A non-numeric namespace still derives the project and
# seed file, but its ports must be set explicitly. Diagnostics go to
# stderr; stdout stays pure so eval never runs a surprise.
set -euo pipefail

fail() {
  echo "[parity-env] $1" >&2
  exit 2
}

# Blank counts as unset.
trim() {
  local v="$1"
  v="${v#"${v%%[![:space:]]*}"}"
  v="${v%"${v##*[![:space:]]}"}"
  printf '%s' "$v"
}

# POSIX-safe single-quote wrapping for eval.
sq() {
  printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"
}

STACK_DIR="$(cd "$(dirname "$0")" && pwd)"
PARITY_DIR="$(cd "$STACK_DIR/.." && pwd)"

NS="$(trim "${PARITY_NS:-}")"
if [ -n "$NS" ]; then
  [ "${#NS}" -le 32 ] || fail "PARITY_NS must be 1-32 chars of letters, digits, \"_\" or \"-\", starting alnum; got '$NS'."
  case "$NS" in
    *[!A-Za-z0-9_-]* | [-_]* | "") fail "PARITY_NS must be 1-32 chars of letters, digits, \"_\" or \"-\", starting alnum; got '$NS'." ;;
  esac
  NS="$(printf '%s' "$NS" | tr '[:upper:]' '[:lower:]')"
fi

PROJECT="$(trim "${PARITY_PROJECT:-}")"
if [ -z "$PROJECT" ]; then
  if [ -n "$NS" ]; then PROJECT="parity$NS"; else PROJECT="parity19"; fi
fi
PREFIX="$(trim "${PARITY_CONTAINER_PREFIX:-}")"
[ -n "$PREFIX" ] || PREFIX="$PROJECT"

# Two-digit port suffix for a numeric namespace (last two digits,
# zero-padded). Suffixes 00 and 19 reproduce the default stack's own
# ports and are rejected; a non-numeric namespace derives no ports.
SUFFIX=""
if [ -n "$NS" ]; then
  case "$NS" in
    *[!0-9]*)
      SUFFIX=""
      ;;
    *)
      # Last two digits, zero-padded (portable: no ${var: -2}, which
      # needs bash 4.2; macOS still ships 3.2).
      SUFFIX="$NS"
      while [ "${#SUFFIX}" -gt 2 ]; do SUFFIX="${SUFFIX#?}"; done
      [ "${#SUFFIX}" -eq 2 ] || SUFFIX="0$SUFFIX"
      if [ "$SUFFIX" = "00" ] || [ "$SUFFIX" = "19" ]; then
        fail "PARITY_NS='$NS' ends in \"$SUFFIX\", which reproduces the default stack's ports; pick a namespace with a different last-two-digits, or set the PARITY_*_PORT variables explicitly."
      fi
      ;;
  esac
fi

resolve_port() {
  # $1 = service key for messages, $2 = env var name, $3 = base, $4 = legacy default
  local explicit base default
  explicit="$(trim "${!2:-}")"
  base="$3"
  default="$4"
  if [ -n "$explicit" ]; then
    case "$explicit" in
      *[!0-9]* | "") fail "$2 must be a TCP port (1-65535), got '$explicit'." ;;
    esac
    [ "$explicit" -ge 1 ] && [ "$explicit" -le 65535 ] \
      || fail "$2 must be a TCP port (1-65535), got '$explicit'."
    printf '%s' "$explicit"
  elif [ -n "$SUFFIX" ]; then
    printf '%s' "$((10#$base + 10#$SUFFIX))"
  elif [ -n "$NS" ]; then
    fail "PARITY_NS='$NS' is not numeric, so ports cannot be derived; set $2 explicitly (or use a numeric namespace)."
  else
    printf '%s' "$default"
  fi
}

ORACLE_PORT="$(resolve_port oracle PARITY_ORACLE_PORT 13000 13000)"
LIVE_PORT="$(resolve_port live PARITY_LIVE_PORT 13100 13001)"
PG_PORT="$(resolve_port pg PARITY_PG_PORT 15400 15419)"
REDIS_PORT="$(resolve_port redis PARITY_REDIS_PORT 16300 16319)"
API_PORT="$(resolve_port api PARITY_API_PORT 18000 18019)"
MINIO_PORT="$(resolve_port minio PARITY_MINIO_PORT 19000 19019)"

SEED_FILE="$(trim "${PARITY_SEED_FILE:-}")"
if [ -z "$SEED_FILE" ]; then
  if [ -n "$NS" ]; then SEED_FILE="$PARITY_DIR/.seed-$NS.json"; else SEED_FILE="$PARITY_DIR/.seed.json"; fi
fi
API_URL="$(trim "${PARITY_API_URL:-}")"
[ -n "$API_URL" ] || API_URL="http://localhost:$API_PORT"
ORACLE_URL="$(trim "${PARITY_ORACLE_URL:-}")"
[ -n "$ORACLE_URL" ] || ORACLE_URL="http://localhost:$ORACLE_PORT"

if [ -n "$NS" ]; then
  echo "[parity-env] namespace '$NS' -> project $PROJECT (oracle :$ORACLE_PORT api :$API_PORT pg :$PG_PORT)" >&2
else
  echo "[parity-env] no namespace; using the default $PROJECT stack" >&2
fi

{
  printf 'export PARITY_NS=%s\n' "$(sq "$NS")"
  printf 'export PARITY_PROJECT=%s\n' "$(sq "$PROJECT")"
  printf 'export PARITY_CONTAINER_PREFIX=%s\n' "$(sq "$PREFIX")"
  printf 'export PARITY_ORACLE_PORT=%s\n' "$(sq "$ORACLE_PORT")"
  printf 'export PARITY_LIVE_PORT=%s\n' "$(sq "$LIVE_PORT")"
  printf 'export PARITY_PG_PORT=%s\n' "$(sq "$PG_PORT")"
  printf 'export PARITY_REDIS_PORT=%s\n' "$(sq "$REDIS_PORT")"
  printf 'export PARITY_API_PORT=%s\n' "$(sq "$API_PORT")"
  printf 'export PARITY_MINIO_PORT=%s\n' "$(sq "$MINIO_PORT")"
  printf 'export PARITY_SEED_FILE=%s\n' "$(sq "$SEED_FILE")"
  printf 'export PARITY_API_URL=%s\n' "$(sq "$API_URL")"
  printf 'export PARITY_ORACLE_URL=%s\n' "$(sq "$ORACLE_URL")"
}
