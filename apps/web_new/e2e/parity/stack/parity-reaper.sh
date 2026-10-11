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
    # and never looks like work in progress inside the checkout.
    (cd / && setsid nohup "$STACK_DIR/parity-reaper.sh" watch "$AGENT" > /dev/null 2>&1 &
      echo $! > "$MARK")
    echo "[parity] stack $PARITY_PROJECT will be torn down when this run (pid $AGENT) ends"
    ;;
  watch)
    AGENT="${2:?usage: parity-reaper.sh watch <pid>}"
    while kill -0 "$AGENT" 2>/dev/null; do
      sleep 15
    done
    "$STACK_DIR/parity-down.sh" > /dev/null 2>&1 \
      || docker compose -p "$PARITY_PROJECT" down -v --remove-orphans > /dev/null 2>&1
    rm -f "${TMPDIR:-/tmp}/parity-reaper.$PARITY_PROJECT.$AGENT.pid"
    ;;
  *)
    echo "usage: parity-reaper.sh arm | watch <pid>" >&2
    exit 2
    ;;
esac
