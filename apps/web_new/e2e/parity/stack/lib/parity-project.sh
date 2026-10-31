# Parity compose-project guard (NEWFRONT-174). Sourced by the stack scripts,
# never executed directly. Two jobs:
#
#   1. Resolve which compose project a destructive script acts on, without
#      ever letting a lost identity silently fall back to the default project.
#      Background: `export PARITY_NS=...` (or PARITY_PROJECT) followed by
#      `sudo <stack script>` drops the variables (sudo env_reset), the
#      resolver falls back to `name: parity19`, and `down -v` deletes a
#      sibling slot's live stack. An explicit project argument survives sudo;
#      exported env does not. So destructive scripts take the project as an
#      argument and refuse to guess under sudo.
#
#   2. Assert, before any `down -v`, that every container the target project
#      owns is inside this stack's expected container-name prefix. A project
#      holding foreign-named containers is a mis-scoped teardown; refuse it
#      rather than reap what may be a sibling's work.
#
# Sourced by the destructive scripts (down/reset) under `set -euo pipefail`;
# keep it safe under plain `set -u` too. Fails closed: any unexpected state
# refuses.
#
# Usage in a destructive script (down/reset):
#   . "$STACK_DIR/lib/parity-project.sh"
#   if [ -n "${1:-}" ]; then
#     PARITY_PROJECT="$(parity_resolve_project "$1")"   # argv survives sudo
#   else
#     parity_require_identity_under_sudo "parity-down.sh <project>" || exit 1
#     PARITY_ENV_OUT="$(parity-env.sh)" || exit $?; eval "$PARITY_ENV_OUT"
#   fi
#   parity_assert_project "$PARITY_PROJECT"
#   docker compose -p "$PARITY_PROJECT" down -v --remove-orphans
#
# Bring-up needs no strictness and does not source this file: landing in the
# wrong project fails loudly on ports/names instead of deleting data, and
# the preflight guard refuses collisions.

# Resolve the compose project to act on. Prints the project name.
#   parity_resolve_project [explicit-project]
# Precedence: explicit argument > PARITY_PROJECT env > (refuse under sudo) >
# default parity19. Destructive scripts call this only on the explicit-argv
# path; the no-argument path resolves through parity-env.sh (PARITY_NS-aware)
# after parity_require_identity_under_sudo.
parity_resolve_project() {
  local explicit="${1:-}"
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
  if { [ -n "${SUDO_USER:-}" ] || [ -n "${SUDO_UID:-}" ]; }; then
    echo "[parity] refusing: running under sudo with no project set, and sudo drops exported PARITY_PROJECT (env_reset), so the default would be a guess." >&2
    echo "[parity] pass the project explicitly: parity-down.sh <project>  (or: sudo env PARITY_PROJECT=<project> parity-down.sh)" >&2
    return 1
  fi
  printf 'parity19\n'
}

# Assert the target project owns no foreign-named containers, running or
# stopped (`down -v` removes both, so `ps` must list both). Returns 0 when
# the project is empty (nothing to destroy) or every container name starts
# with the expected prefix. Refuses otherwise, and refuses when the project
# cannot even be listed (fail closed: an unreachable daemon must never read
# as "nothing there"). PARITY_ALLOW_NAME_SKEW=1 downgrades a skew refusal to
# a warning, for recovering a stack whose names drifted from its project.
parity_assert_project() {
  local project="${1:?usage: parity_assert_project <project>}"
  local prefix="${PARITY_CONTAINER_PREFIX:-$project}"
  local names
  if ! names="$(docker compose -p "$project" ps --all --format '{{.Name}}' 2>/dev/null)"; then
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

# Refuse when running under sudo with no usable stack identity. sudo drops
# exported PARITY_NS / PARITY_PROJECT (env_reset), so resolving through
# parity-env.sh now would silently fall back to the default stack — the
# NEWFRONT-174 incident shape. Destructive scripts call this on the
# no-argument path, before the resolver. When identity survives (plain env,
# or `sudo env PARITY_NS=...`, or a sudoers env_keep), this passes and
# resolution proceeds.
#   parity_require_identity_under_sudo <example>
# Prints the refusal with <example> as the suggested fix.
parity_require_identity_under_sudo() {
  local example="${1:?usage: parity_require_identity_under_sudo <example>}"
  if [ -n "${PARITY_NS:-}" ] || [ -n "${PARITY_PROJECT:-}" ]; then
    return 0
  fi
  if { [ -n "${SUDO_USER:-}" ] || [ -n "${SUDO_UID:-}" ]; }; then
    echo "[parity] refusing: running under sudo with no project set, and sudo drops exported PARITY_NS / PARITY_PROJECT (env_reset), so the default would be a guess." >&2
    echo "[parity] pass the project explicitly: $example" >&2
    return 1
  fi
  return 0
}
