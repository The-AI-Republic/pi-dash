# Parity compose-project guard (NEWFRONT-174). Sourced by the stack scripts,
# never executed directly. Two jobs:
#
#   1. Resolve which compose project a script acts on, without ever letting
#      a lost PARITY_PROJECT silently fall back to the default project on a
#      destructive path. Background: `export PARITY_PROJECT=...` followed by
#      `sudo docker compose ...` drops the variable (sudo env_reset), compose
#      falls back to `name: parity19`, and `down -v` deletes a sibling slot's
#      live stack. An explicit project argument survives sudo; exported env
#      does not. So destructive scripts take the project as an argument and
#      refuse to guess under sudo.
#
#   2. Assert, before any `down -v`, that every container the target project
#      owns is inside this stack's expected container-name prefix. A project
#      holding foreign-named containers is a mis-scoped teardown; refuse it
#      rather than reap what may be a sibling's work.
#
# Must stay safe under both `set -euo pipefail` (up/down/reset) and plain
# `set -u` (reaper). Fails closed: any unexpected state refuses.
#
# Usage in a script:
#   . "$STACK_DIR/lib/parity-project.sh"
#   PARITY_PROJECT="$(parity_resolve_project "${1:-}")"   # destructive path
#   parity_assert_project "$PARITY_PROJECT"
#   docker compose -p "$PARITY_PROJECT" down -v --remove-orphans

# Resolve the compose project to act on. Prints the project name.
#   parity_resolve_project [explicit-project]
# Precedence: explicit argument > PARITY_PROJECT env > (refuse under sudo) >
# default parity19. The strict form is for destructive paths. Bring-up uses
# parity_resolve_project_allow_sudo, which keeps the old silent default under
# sudo: landing in the wrong project fails loudly on ports/names instead of
# deleting data. Two functions (not a flag) so a caller can never smuggle the
# lenient mode in through an argument.
parity_resolve_project() {
  _parity_resolve_project 0 "${1:-}"
}

parity_resolve_project_allow_sudo() {
  _parity_resolve_project 1 "${1:-}"
}

_parity_resolve_project() {
  local allow_sudo_default="$1"
  local explicit="${2:-}"
  case "$explicit" in
    -*) echo "[parity] refusing: invalid project '$explicit'" >&2; return 1 ;;
  esac
  local from_env="${PARITY_PROJECT:-}"
  if [ -n "$explicit" ]; then
    if [ -n "$from_env" ] && [ "$from_env" != "$explicit" ]; then
      echo "[parity] refusing: explicit project '$explicit' disagrees with PARITY_PROJECT='$from_env' in the environment" >&2
      return 1
    fi
    printf '%s\n' "$explicit"
    return 0
  fi
  if [ -n "$from_env" ]; then
    printf '%s\n' "$from_env"
    return 0
  fi
  if [ "$allow_sudo_default" = "0" ] && { [ -n "${SUDO_USER:-}" ] || [ -n "${SUDO_UID:-}" ]; }; then
    echo "[parity] refusing: running under sudo with no project set, and sudo drops exported PARITY_PROJECT (env_reset), so the default would be a guess." >&2
    echo "[parity] pass the project explicitly: parity-down.sh <project>  (or: sudo env PARITY_PROJECT=<project> parity-down.sh)" >&2
    return 1
  fi
  printf 'parity19\n'
}

# Assert the target project owns no foreign-named containers. Returns 0 when
# the project is empty (nothing to destroy) or every container name starts
# with the expected prefix. Refuses otherwise, and refuses when the project
# cannot even be listed (fail closed: an unreachable daemon must never read
# as "nothing there"). PARITY_ALLOW_NAME_SKEW=1 downgrades a skew refusal to
# a warning, for recovering a stack whose names drifted from its project.
parity_assert_project() {
  local project="${1:?usage: parity_assert_project <project>}"
  local prefix="${PARITY_CONTAINER_PREFIX:-$project}"
  local names
  if ! names="$(docker compose -p "$project" ps --format '{{.Name}}' 2>/dev/null)"; then
    echo "[parity] refusing: cannot list containers for project '$project' (is the Docker daemon reachable?)" >&2
    return 1
  fi
  if [ -z "$names" ]; then
    return 0
  fi
  local bad=0
  local name
  while IFS= read -r name; do
    [ -z "$name" ] && continue
    case "$name" in
      "$prefix"-*) ;;
      *)
        echo "[parity] project '$project' owns '$name', outside expected prefix '$prefix-'" >&2
        bad=1
        ;;
    esac
  done <<< "$names"
  if [ "$bad" != "0" ]; then
    if [ "${PARITY_ALLOW_NAME_SKEW:-0}" = "1" ]; then
      echo "[parity] WARNING: proceeding despite name skew (PARITY_ALLOW_NAME_SKEW=1)" >&2
      return 0
    fi
    echo "[parity] refusing: set PARITY_CONTAINER_PREFIX to match, or PARITY_ALLOW_NAME_SKEW=1 to override" >&2
    return 1
  fi
  return 0
}
