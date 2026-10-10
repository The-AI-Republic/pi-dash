#!/usr/bin/env bash
# Parity-stack ownership + port guard (NEWFRONT-132). Usage:
#
#   parity-guard.sh <project> <stack-dir> [--ports p1,p2,...] [--warn]
#
# Refuses (exit 3) when the compose project already has containers created
# from a different checkout (compared by the working_dir label docker
# keeps): bringing up or resetting here would reseed or destroy a
# sibling's stack. With --ports, also refuses (exit 1) when any listed
# port is held by something other than this project's own containers.
# With --warn, every finding becomes a stderr warning and the exit status
# stays 0 (for the teardown path, which is also the deliberate override).
set -euo pipefail

PROJECT="${1:?usage: parity-guard.sh <project> <stack-dir> [--ports p1,p2,...] [--warn]}"
STACK_DIR="${2:?usage: parity-guard.sh <project> <stack-dir> [--ports p1,p2,...] [--warn]}"
WANT_PORTS=""
WARN=0
shift 2
while [ "$#" -gt 0 ]; do
  case "$1" in
    --ports) WANT_PORTS="${2:?--ports needs a value}"; shift 2 ;;
    --warn) WARN=1; shift ;;
    *) echo "[parity-guard] unknown argument: $1" >&2; exit 2 ;;
  esac
done

PS_JSON="$(docker compose -p "$PROJECT" ps -a --format json 2>/dev/null || true)"
export PS_JSON PROJECT STACK_DIR WANT_PORTS WARN
python3 - <<'EOF'
import json
import os
import socket
import sys
import time

project = os.environ["PROJECT"]
stack_dir = os.path.realpath(os.environ["STACK_DIR"])
want = [p for p in os.environ["WANT_PORTS"].split(",") if p]
warn = os.environ["WARN"] == "1"

foreign_dirs = set()
own_ports = set()
for line in os.environ["PS_JSON"].splitlines():
    line = line.strip()
    if not line:
        continue
    try:
        item = json.loads(line)
    except ValueError:
        continue
    labels = {}
    for chunk in str(item.get("Labels") or "").split(","):
        key, sep, value = chunk.partition("=")
        if sep:
            labels[key] = value
    working_dir = labels.get("com.docker.compose.project.working_dir", "")
    owned = bool(working_dir) and os.path.realpath(working_dir) == stack_dir
    if not owned:
        # No label at all (foreign tooling?) fails closed: treated as
        # another checkout's container.
        foreign_dirs.add(working_dir if working_dir else "<unknown>")
        continue
    for pub in item.get("Publishers") or []:
        if pub.get("PublishedPort"):
            own_ports.add(str(pub["PublishedPort"]))

def report(message, code):
    print(message, file=sys.stderr)
    if not warn:
        sys.exit(code)

if foreign_dirs:
    dirs = ", ".join(sorted(foreign_dirs))
    tail = (
        "Proceeding anyway (--warn)."
        if warn
        else "Nothing was built, started or destroyed. If that checkout is gone and the stack "
        "is orphaned, remove it deliberately with: docker compose -p %s down -v --remove-orphans" % project
    )
    report(
        "[parity] project %s already has containers from another checkout (%s). "
        "This PARITY_NS collides with a live stack: pick a free namespace "
        "(`docker ps` shows who owns what). %s" % (project, dirs, tail),
        3,
    )

busy = []
for port in want:
    if port in own_ports:
        continue
    try:
        number = int(port)
    except ValueError:
        busy.append(port)
        continue
    # SO_REUSEADDR like docker itself: a leftover TIME_WAIT from a just
    # closed connection must not read as busy. Three attempts five seconds
    # apart absorb a teardown that is still releasing its ports.
    ok = False
    for _ in range(3):
        sock = socket.socket()
        try:
            sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            sock.bind(("0.0.0.0", number))
            ok = True
        except OSError:
            time.sleep(5)
        finally:
            sock.close()
        if ok:
            break
    if not ok:
        busy.append(port)
if busy:
    report(
        "[parity] refusing to start: port(s) %s already in use. Another stack owns them "
        "(each namespace needs distinct last-two-digits), or a foreign process does. "
        "See who with `docker ps`, then pick a free PARITY_NS or set the PARITY_*_PORT "
        "variables explicitly. Nothing was built or started." % ", ".join(busy),
        1,
    )
EOF
