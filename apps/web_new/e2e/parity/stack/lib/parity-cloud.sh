# Cloud-oracle overlay helpers (NEWFRONT-436). Sourced by the stack scripts,
# never executed directly. Two jobs:
#
#   1. Resolve the runner-local overlay checkout: $CLOUD_OVERLAY_DIR wins,
#      else the <repo-root>/../private-pi-dash sibling (the RUNNING-LOCALLY
#      layout). The checkout lives OUTSIDE the pi-dash tree and is treated
#      as a read-only input: these helpers only ever copy FROM it.
#
#   2. Stage the overlay inputs the cloud images need into the stack's own
#      .cloud-staging/ directory (gitignored, refreshed on every cloud
#      bring-up, removed by parity-down.sh). Staging exists because the
#      runner hosts invoke docker through a sudo wrapper that forwards only
#      the PARITY_*/ORACLE_UPSTREAM/LIVE_UPSTREAM/COMPOSE_* variables, so a
#      compose-interpolated overlay path would silently render empty;
#      static in-repo staging paths need no new variables, and `docker
#      compose config` stays green in CI where no overlay exists.
#
# Sourced under `set -euo pipefail`; keep it safe under plain `set -u` too.
# Fails closed: a missing or incomplete overlay refuses with setup
# instructions instead of building a half-cloud image.
#
# Usage in parity-up.sh (cloud edition only):
#   . "$STACK_DIR/lib/parity-cloud.sh"
#   CLOUD_OVERLAY_DIR="$(parity_cloud_resolve_overlay)" || exit 1
#   parity_cloud_stage_overlay "$CLOUD_OVERLAY_DIR" || exit 1
#
# Usage in parity-down.sh (every teardown, best-effort):
#   . "$STACK_DIR/lib/parity-project.sh"   # already sourced
#   . "$STACK_DIR/lib/parity-cloud.sh"
#   parity_cloud_clean_staging || true

# Resolve the overlay checkout directory. Prints the absolute path.
# Precedence: $CLOUD_OVERLAY_DIR > <repo-root>/../private-pi-dash.
# Refuses when the directory is missing or lacks the subtrees the cloud
# images copy (a shallow clone that skipped them, or a wrong directory).
parity_cloud_resolve_overlay() {
  local dir="${CLOUD_OVERLAY_DIR:-}"
  if [ -z "$dir" ]; then
    dir="$(cd "${STACK_DIR:?parity-cloud.sh needs STACK_DIR}/../../../../.." && pwd)/../private-pi-dash"
  fi
  if [ ! -d "$dir" ]; then
    parity_cloud_missing "$dir"
    return 1
  fi
  local missing=0
  local sub
  # ee-overlay/apps/admin is the one-file cloud home-page override the
  # cloud admin oracle swaps in (NEWFRONT-446); the checkout carries it
  # next to the web/api subtrees, so requiring it breaks no existing
  # checkout.
  for sub in ee-overlay/apps/web ee-overlay/apps/api ee-overlay/apps/admin ee-overlay/packages pi_dash_cloud .oss-pin; do
    if [ ! -e "$dir/$sub" ]; then
      echo "[parity] cloud overlay at '$dir' is missing '$sub'" >&2
      missing=1
    fi
  done
  if [ "$missing" != "0" ]; then
    echo "[parity] is '$dir' a full private-pi-dash checkout? See the Cloud oracle runbook in stack/README.md." >&2
    return 1
  fi
  (cd "$dir" && pwd)
}

parity_cloud_missing() {
  local dir="${1:?usage: parity_cloud_missing <dir>}"
  echo "[parity] cloud edition needs the private overlay checkout at '$dir', and it is not there." >&2
  echo "[parity] clone it OUTSIDE the pi-dash tree (read-only input; never commit it into pi-dash):" >&2
  echo "[parity]   git clone --depth 1 git@github.com:The-AI-Republic/private-pi-dash.git <checkout>" >&2
  echo "[parity] then either place it at '$dir' or export CLOUD_OVERLAY_DIR=<checkout>." >&2
}

# Copy the overlay inputs the cloud images need into .cloud-staging/.
# Refreshed (not merged) on every call, so overlay edits apply on the next
# bring-up and deletions never linger. Prints provenance for the log.
parity_cloud_stage_overlay() {
  local dir="${1:?usage: parity_cloud_stage_overlay <overlay-dir>}"
  local staging="${STACK_DIR:?parity-cloud.sh needs STACK_DIR}/.cloud-staging"
  rm -rf "$staging"
  mkdir -p "$staging/ee-overlay/apps" "$staging/ee-overlay/packages"
  cp -a "$dir/ee-overlay/apps/web" "$staging/ee-overlay/apps/web"
  cp -a "$dir/ee-overlay/apps/api" "$staging/ee-overlay/apps/api"
  cp -a "$dir/ee-overlay/apps/admin" "$staging/ee-overlay/apps/admin"
  cp -a "$dir/ee-overlay/packages" "$staging/ee-overlay/packages"
  cp -a "$dir/pi_dash_cloud" "$staging/pi_dash_cloud"
  local head="unknown"
  head="$(git -C "$dir" rev-parse --short HEAD 2>/dev/null || echo unknown)"
  local pin="unknown"
  pin="$(cat "$dir/.oss-pin" 2>/dev/null || echo unknown)"
  echo "[parity] cloud overlay staged: $dir (overlay HEAD $head, pinned OSS $pin)"
}

# Remove the staging directory. Best-effort: teardown must succeed with or
# without a staged tree (plain OSS stacks never create one).
parity_cloud_clean_staging() {
  rm -rf "${STACK_DIR:?parity-cloud.sh needs STACK_DIR}/.cloud-staging"
}
