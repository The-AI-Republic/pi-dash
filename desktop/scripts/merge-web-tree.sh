#!/usr/bin/env bash
# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Builds the merged web workspace the desktop app bundles:
#
#   merge-web-tree.sh <dest> [extra-dir ...]
#
# Layers, merged in order (last writer wins):
#   1. this pi-dash checkout (working tree as-is)
#   2. each directory in PIDASH_DESKTOP_EXTRA_OVERLAYS (colon-separated),
#      in order — how an edition layers its own overrides under the
#      desktop ones. Unset for the plain OSS build.
#   3. desktop-overlay/
#   4. each extra-dir argument, in order (e.g. edition-only test files)
#
# node_modules/ in <dest> is preserved across runs so pnpm install stays
# incremental. Shared by dev-prep.sh and test-overlay.sh so bundles and
# tests always see the same tree.

set -euo pipefail

SCRIPT_DIR="$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
OSS_DIR="$( dirname "$( dirname "$SCRIPT_DIR" )" )"

if [[ $# -lt 1 ]]; then
    echo "usage: $0 <dest> [extra-dir ...]" >&2
    exit 2
fi
DEST="$1"
shift

if ! command -v rsync >/dev/null 2>&1; then
    echo "[merge-web-tree] ERROR: rsync is required but was not found in PATH." >&2
    exit 1
fi

OVERLAYS=()
if [[ -n "${PIDASH_DESKTOP_EXTRA_OVERLAYS:-}" ]]; then
    IFS=':' read -r -a OVERLAYS <<< "$PIDASH_DESKTOP_EXTRA_OVERLAYS"
fi
OVERLAYS+=("$OSS_DIR/desktop-overlay")
OVERLAYS+=("$@")
for overlay in "${OVERLAYS[@]}"; do
    if [[ ! -d "$overlay" ]]; then
        echo "[merge-web-tree] ERROR: overlay is not a directory: $overlay" >&2
        exit 1
    fi
done

mkdir -p "$DEST"

# Mirror this checkout into <dest>, excluding worktree-only state and
# generated artifacts. desktop/ (which holds the default <dest> itself) and
# desktop-overlay/ are not part of the web workspace and are excluded so the
# copy never recurses into its own destination. node_modules/ is preserved
# in dest (the --exclude means rsync doesn't touch it at either end).
#
# Patterns starting with '/' are anchored to the source root; bare names
# match at any depth. The destructive artifacts (target/, apps/*/build/,
# apps/*/dist/, apps/*/.react-router/) are anchored so we DON'T accidentally
# drop legitimate sources or workspace-package outputs (packages/*/dist/
# contains tsdown builds that other workspace packages import via
# package.json#exports — losing them breaks the web build).
echo "[merge-web-tree] Syncing $OSS_DIR → $DEST"
rsync -a --delete \
    --exclude='/desktop/' \
    --exclude='/desktop-overlay/' \
    --exclude='node_modules/' \
    --exclude='.git/' \
    --exclude='.turbo/' \
    --exclude='.pnpm-store/' \
    --exclude='/target/' \
    --exclude='/apps/*/build/' \
    --exclude='/apps/*/dist/' \
    --exclude='/apps/*/.react-router/' \
    "$OSS_DIR/" "$DEST/"

for overlay in "${OVERLAYS[@]}"; do
    echo "[merge-web-tree] Applying $overlay"
    # An overlay's top-level README.md documents the overlay itself; it must
    # not replace the workspace README.
    rsync -a --exclude='.gitkeep' --exclude='/README.md' "$overlay/" "$DEST/"
done

# Sanity: the web app must exist after overlay merge.
test -f "$DEST/apps/web/package.json"
