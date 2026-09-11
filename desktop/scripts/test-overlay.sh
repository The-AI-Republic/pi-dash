#!/usr/bin/env bash
# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Runs the desktop web tests (apps/web/tests/desktop/) against the same
# merged tree the desktop bundles:
#
#   test-overlay.sh [extra-dir ...]
#
# PIDASH_DESKTOP_EXTRA_OVERLAYS is honoured exactly as in dev-prep.sh, and
# each extra-dir is applied last — an edition passes its own test files that
# way. Shares desktop/.dev-tree/ (or PIDASH_DESKTOP_DEV_TREE) and its
# node_modules with dev-prep.sh.

set -euo pipefail

SCRIPT_DIR="$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
DESKTOP_DIR="$( dirname "$SCRIPT_DIR" )"
DEV_TREE="${PIDASH_DESKTOP_DEV_TREE:-$DESKTOP_DIR/.dev-tree}"

bash "$SCRIPT_DIR/merge-web-tree.sh" "$DEV_TREE" "$@"

cd "$DEV_TREE"
if command -v corepack >/dev/null 2>&1; then
    corepack enable >/dev/null 2>&1 || true
    corepack prepare --activate >/dev/null 2>&1 || true
fi

echo "[test-overlay] pnpm install"
pnpm install --frozen-lockfile

# --force: .dev-tree/ is gitignored, so turbo's git-based hashing cannot see
# what the overlays changed and would replay a cached (possibly stale) run.
echo "[test-overlay] turbo run test --filter=web -- tests/desktop"
pnpm exec turbo run test --filter=web --force -- tests/desktop
