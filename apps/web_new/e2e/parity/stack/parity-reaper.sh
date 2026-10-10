#!/usr/bin/env bash
# Tears the parity stack down when the Pi Dash agent run that started it ends,
# however it ends. A stack is 0.5-1 GB of memory; left behind after every run
# they exhaust the runner host.
#
#   parity-reaper.sh arm            called by parity-up.sh
#   parity-reaper.sh watch <pid>    the detached watcher `arm` starts
#
# `arm` finds the agent process of the current run (the ancestor started by
# the `pidash` runner daemon) and starts one watcher per stack and run. The
# watcher waits for that process to exit, then runs parity-down.sh. Outside a
# Pi Dash run (a person's shell, CI) there is no such ancestor and `arm` does
# nothing. Set PARITY_KEEP_STACK=1 to opt out.
set -u

export STACK_DIR
STACK_DIR="$(cd "$(dirname "$0")" && pwd)"
# Namespaced stacks (NEWFRONT-132): resolve PARITY_NS to the project the
# same way the other scripts do. If the resolver is gone (checked-out
# stack removed out from under the watcher) fall back to the ambient
# project, which parity-up.sh exported when it armed this watcher.
if [ -z "${PARITY_PROJECT:-}" ] && [ -n "${PARITY_NS:-}" ] && [ -x "$STACK_DIR/parity-env.sh" ]; then
  # Two steps on purpose: eval'ing the substitution directly would mask a
  # resolver failure (eval succeeds on empty input).
  export PARITY_ENV_OUT
  PARITY_ENV_OUT="$("$STACK_DIR/parity-env.sh" 2>/dev/null)" || exit 2
  eval "$PARITY_ENV_OUT"
fi
export PARITY_PROJECT
PARITY_PROJECT="${PARITY_PROJECT:-parity19}"

# Print the pid of the process the runner daemon started for this run.
agent_pid() {
  local pid=$$ ppid comm
  while [ "$pid" -gt 1 ]; do
    ppid="$(awk '/^PPid:/{print $2}' "/proc/$pid/status" 2>/dev/null)" || return 1
    [ -n "$ppid" ] || return 1
    comm="$(cat "/proc/$ppid/comm" 2>/dev/null)" || return 1
    if [ "$comm" = "pidash" ]; then
      echo "$pid"
      return 0
    fi
    pid="$ppid"
  done
  return 1
}

case "${1:-}" in
  arm)
    [ "${PARITY_KEEP_STACK:-0}" = "1" ] && exit 0
    AGENT="$(agent_pid)" || exit 0
    MARK="${TMPDIR:-/tmp}/parity-reaper.$PARITY_PROJECT.$AGENT.pid"
    if [ -f "$MARK" ] && kill -0 "$(cat "$MARK")" 2>/dev/null; then
      exit 0
    fi
    # Own session and cwd /, so the watcher outlives the agent's process group
    # and never looks like work in progress inside the checkout. setsid --fork
    # makes the spawned parent exit at once: without it setsid execs the
    # watcher in place (no fork when it is not a process-group leader), the
    # background job blocks in waitpid until the run ends, and its inherited
    # stdout keeps any `parity-up.sh ... | tail` bring-up from ever seeing EOF
    # (NEWFRONT-350). No `&` job is left behind at all.
    if (cd / && setsid --fork nohup "$STACK_DIR/parity-reaper.sh" watch "$AGENT" \
        </dev/null >/dev/null 2>&1); then
      # The watcher announces its own pid in MARK on startup ($! would only be
      # the short-lived fork parent). Wait briefly so a back-to-back arm sees
      # the live watcher, but exit promptly either way.
      for _ in $(seq 1 50); do
        WATCHER="$(cat "$MARK" 2>/dev/null || true)"
        if [ -n "$WATCHER" ] && kill -0 "$WATCHER" 2>/dev/null; then
          break
        fi
        sleep 0.1
      done
    fi
    echo "[parity] stack $PARITY_PROJECT will be torn down when this run (pid $AGENT) ends"
    ;;
  watch)
    AGENT="${2:?usage: parity-reaper.sh watch <pid>}"
    # Announce this watcher's pid so `arm` (which waits on MARK) and later
    # arms track the watcher itself rather than a spawn intermediate.
    echo "$$" > "${TMPDIR:-/tmp}/parity-reaper.$PARITY_PROJECT.$AGENT.pid"
    while kill -0 "$AGENT" 2>/dev/null; do
      sleep 15
    done
    # Explicit project argument: argv survives sudo and env loss, exported
    # PARITY_PROJECT does not (NEWFRONT-174). The fallback keeps -p for the
    # same reason when the checkout (and parity-down.sh) is already gone —
    # and only then: a guard refusal (exit 1 with the script present) must
    # never be overridden by an unguarded down -v.
    "$STACK_DIR/parity-down.sh" "$PARITY_PROJECT" > /dev/null 2>&1
    rc=$?
    if [ "$rc" -ne 0 ] && [ ! -x "$STACK_DIR/parity-down.sh" ]; then
      docker compose -p "$PARITY_PROJECT" down -v --remove-orphans > /dev/null 2>&1 || true
    fi
    rm -f "${TMPDIR:-/tmp}/parity-reaper.$PARITY_PROJECT.$AGENT.pid"
    ;;
  *)
    echo "usage: parity-reaper.sh arm | watch <pid>" >&2
    exit 2
    ;;
esac
