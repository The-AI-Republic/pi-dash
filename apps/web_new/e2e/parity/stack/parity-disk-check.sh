#!/usr/bin/env bash
# Pre-sweep disk guard for the parity harness (NEWFRONT-175).
#
# Refuses to start a sweep when the host disk is nearly full: at 100% any
# slot's Postgres PANICs with "No space left on device" mid-run, burning the
# sweep and risking an unclean DB shutdown. parity-up.sh runs this before the
# docker build; run it by hand before a long sweep against a kept stack:
#
#   apps/web_new/e2e/parity/stack/parity-disk-check.sh [--path <dir>]...
#
# Checks the filesystem holding / plus the filesystem holding Docker's
# data-root (when the daemon answers), since either one filling up kills the
# slot's Postgres. The limits are absolute free space, not a percentage, so a
# large disk is not refused while it still has room for many stacks: warns
# below PARITY_DISK_WARN_FREE_GB (default 15), refuses below
# PARITY_DISK_MIN_FREE_GB (default 6, the old 90% limit on a 60 GB host).
# PARITY_IGNORE_DISK_CHECK=1 bypasses
# the refusal for emergencies; never use it on a shared host without the
# operator's acknowledgement.
#
# Exit codes: 0 ok (or warn-and-proceed, or bypassed), 1 refused,
# 2 usage/config error.
set -euo pipefail

WARN_GB="${PARITY_DISK_WARN_FREE_GB:-15}"
MIN_GB="${PARITY_DISK_MIN_FREE_GB:-6}"

usage() {
  echo "usage: parity-disk-check.sh [--path <dir>]..."
  echo "Refuses (exit 1) when / or Docker's data-root has less than PARITY_DISK_MIN_FREE_GB free (default 6); warns below PARITY_DISK_WARN_FREE_GB (default 15)."
  echo "PARITY_IGNORE_DISK_CHECK=1 bypasses the refusal for emergencies."
}

EXTRA_PATHS=()
while [ $# -gt 0 ]; do
  case "$1" in
    --path)
      if [ $# -lt 2 ]; then
        usage >&2
        exit 2
      fi
      EXTRA_PATHS+=("$2")
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "[parity] unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

# The percentage limits were replaced by free-space floors; fail loudly
# rather than silently ignore an old override.
for old in PARITY_DISK_WARN_PCT PARITY_DISK_MAX_USE_PCT; do
  if [ -n "${!old:-}" ]; then
    echo "[parity] $old is no longer supported; use PARITY_DISK_WARN_FREE_GB / PARITY_DISK_MIN_FREE_GB (free GB)" >&2
    exit 2
  fi
done
case "$WARN_GB" in ''|*[!0-9]*)
  echo "[parity] PARITY_DISK_WARN_FREE_GB must be a whole number of GB, got '$WARN_GB'" >&2
  exit 2
  ;;
esac
case "$MIN_GB" in ''|*[!0-9]*)
  echo "[parity] PARITY_DISK_MIN_FREE_GB must be a whole number of GB, got '$MIN_GB'" >&2
  exit 2
  ;;
esac
if [ "$MIN_GB" -lt 1 ]; then
  echo "[parity] PARITY_DISK_MIN_FREE_GB must be at least 1, got $MIN_GB" >&2
  exit 2
fi
if [ "$WARN_GB" -le "$MIN_GB" ]; then
  echo "[parity] PARITY_DISK_WARN_FREE_GB ($WARN_GB) must be above PARITY_DISK_MIN_FREE_GB ($MIN_GB)" >&2
  exit 2
fi
MIN_KB=$((MIN_GB * 1048576))
WARN_KB=$((WARN_GB * 1048576))

if [ "${PARITY_IGNORE_DISK_CHECK:-0}" = "1" ]; then
  echo "[parity] disk check BYPASSED via PARITY_IGNORE_DISK_CHECK=1 (emergencies only)"
  exit 0
fi

# usage_for <path>: prints "<use_pct> <avail_1k_blocks> <device>" for the
# filesystem holding <path>, or fails when df cannot read it.
usage_for() {
  local path df_out use avail dev
  path="$1"
  df_out="$(df -P "$path" 2>/dev/null)" || return 1
  use="$(echo "$df_out" | awk 'NR==2 {print $5}')"
  avail="$(echo "$df_out" | awk 'NR==2 {print $4}')"
  dev="$(echo "$df_out" | awk 'NR==2 {print $1}')"
  use="${use%\%}"
  case "$use" in ''|*[!0-9]*) return 1 ;; esac
  case "$avail" in ''|*[!0-9]*) return 1 ;; esac
  echo "$use $avail $dev"
}

# The filesystem with the least free space decides the outcome.
WORST_AVAIL=""
WORST_USE=0
WORST_PATH=""

record_usage() {
  local path use avail dev avail_gb
  path="$1"
  use="$2"
  avail="$3"
  dev="$4"
  avail_gb="$(awk -v a="$avail" 'BEGIN {printf "%.1f", a/1048576}')"
  echo "[parity] disk: $path on $dev at ${use}% used (${avail_gb}G free)"
  if [ -z "$WORST_AVAIL" ] || [ "$avail" -lt "$WORST_AVAIL" ]; then
    WORST_AVAIL="$avail"
    WORST_USE="$use"
    WORST_PATH="$path"
  fi
}

check_path() {
  local path use avail dev info
  path="$1"
  if ! info="$(usage_for "$path")"; then
    echo "[parity] disk check: cannot read disk usage for '$path' (df failed)" >&2
    exit 2
  fi
  read -r use avail dev <<< "$info"
  record_usage "$path" "$use" "$avail" "$dev"
}

# vm_usage_for <path>: like usage_for, but df runs inside a throwaway
# container with no mounts. A container's / sits on the daemon VM's disk,
# so this measures the filesystem that actually holds the data-root when
# the daemon is VM-backed (Docker Desktop on macOS/Windows).
vm_usage_for() {
  local path df_out use avail dev
  path="$1"
  df_out="$(docker run --rm --network none --pull=missing alpine:3.20 df -P / 2>/dev/null)" || return 1
  use="$(echo "$df_out" | awk 'NR==2 {print $5}')"
  avail="$(echo "$df_out" | awk 'NR==2 {print $4}')"
  dev="$(echo "$df_out" | awk 'NR==2 {print $1}')"
  use="${use%\%}"
  case "$use" in ''|*[!0-9]*) return 1 ;; esac
  case "$avail" in ''|*[!0-9]*) return 1 ;; esac
  echo "$use $avail $dev"
}

check_path "/"

DOCKER_ROOT=""
DOCKER_OS=""
if command -v docker >/dev/null 2>&1; then
  DOCKER_INFO="$(docker info --format '{{.OperatingSystem}}|{{.DockerRootDir}}' 2>/dev/null || true)"
  DOCKER_OS="${DOCKER_INFO%%|*}"
  DOCKER_ROOT="${DOCKER_INFO#*|}"
  if [ "$DOCKER_INFO" = "$DOCKER_ROOT" ]; then
    DOCKER_OS=""
  fi
fi
if [ -n "$DOCKER_ROOT" ]; then
  if [ "$DOCKER_ROOT" = "/" ]; then
    echo "[parity] disk: docker data-root is / (same filesystem)"
  elif [ -e "$DOCKER_ROOT" ]; then
    check_path "$DOCKER_ROOT"
  else
    # The data-root is not visible on this host. That is normal for a
    # VM-backed daemon (Docker Desktop on macOS/Windows), where the path
    # exists only inside the daemon's Linux VM; on a Linux host it means a
    # genuinely unreadable path and stays a config error.
    case "$DOCKER_OS" in
      *"Docker Desktop"*) VM_BACKED=1 ;;
      *) VM_BACKED=0 ;;
    esac
    if [ "$(uname -s)" = "Darwin" ]; then
      VM_BACKED=1
    fi
    if [ "$VM_BACKED" = "1" ]; then
      if info="$(vm_usage_for "$DOCKER_ROOT")"; then
        read -r use avail dev <<< "$info"
        echo "[parity] disk: Docker Desktop VM data-root $DOCKER_ROOT measured inside the daemon"
        record_usage "$DOCKER_ROOT" "$use" "$avail" "$dev"
      else
        echo "[parity] disk check WARNING: Docker data-root '$DOCKER_ROOT' lives in a VM and its disk could not be measured; VM disk was not checked, host / check only" >&2
      fi
    else
      echo "[parity] disk check: cannot read disk usage for '$DOCKER_ROOT' (df failed)" >&2
      exit 2
    fi
  fi
else
  echo "[parity] disk: docker not reachable; checked / only"
fi

if [ "${#EXTRA_PATHS[@]}" -gt 0 ]; then
  for extra in "${EXTRA_PATHS[@]}"; do
    check_path "$extra"
  done
fi

WORST_GB="$(awk -v a="$WORST_AVAIL" 'BEGIN {printf "%.1f", a/1048576}')"
if [ "$WORST_AVAIL" -lt "$MIN_KB" ]; then
  echo "[parity] disk check REFUSED: $WORST_PATH has ${WORST_GB}G free, ${WORST_USE}% used (needs ${MIN_GB}G free; warn below ${WARN_GB}G)"
  echo "[parity] refusing to start: a sweep against a full disk panics Postgres (NEWFRONT-175)."
  echo "[parity] read-only diagnosis: df -h / ; df -h /tmp ; docker system df"
  echo "[parity] recovery is the operator's janitoring (NEWFRONT-248): free space, restart the slot Postgres, verify the seed, re-sweep from pristine state."
  echo "[parity] agents must not prune sibling stacks/images/caches or /tmp owned by other runs. Bypass (emergencies only): PARITY_IGNORE_DISK_CHECK=1."
  exit 1
fi
if [ "$WORST_AVAIL" -lt "$WARN_KB" ]; then
  echo "[parity] disk check WARNING: $WORST_PATH has ${WORST_GB}G free, ${WORST_USE}% used (warn below ${WARN_GB}G, refuse below ${MIN_GB}G); proceeding"
  exit 0
fi
echo "[parity] disk check ok: least free ${WORST_GB}G (${WORST_USE}% used) on $WORST_PATH (warn below ${WARN_GB}G, refuse below ${MIN_GB}G)"
exit 0
