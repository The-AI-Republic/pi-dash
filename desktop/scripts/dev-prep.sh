#!/usr/bin/env bash
# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Populates desktop/src-tauri/dist/ with the layered overlay-merged web SPA
# for bundled-dev mode (the default of `cargo tauri dev`) and for local
# `cargo tauri build`. Invoked automatically via tauri.conf.json's
# beforeDevCommand / beforeBuildCommand.
#
# The web tree is assembled by merge-web-tree.sh: this checkout, then any
# PIDASH_DESKTOP_EXTRA_OVERLAYS (an edition's overrides), then
# desktop-overlay/ (last writer wins).
#
# PIDASH_DESKTOP_VERIFY_HOOK, when set, is run with the built client
# directory as its only argument after the generic checks below pass, so an
# edition can assert things about its own bundle.
#
# Build artifacts live in desktop/.dev-tree/ (gitignored; override with
# PIDASH_DESKTOP_DEV_TREE, e.g. to keep an edition's merged tree separate).
# pnpm and turbo caches stay warm across runs, so post-first-run is ~10–30s.
#
# Skip-paths:
#   PIDASH_DESKTOP_HOT_RELOAD=1 — webview opens at PI_DASH_URL directly,
#       dist/ is unused.
#   PIDASH_SKIP_DEV_PREP=1 — caller has already populated dist/ (CI, or
#       a manually-managed local artifact). Cannot verify the SPA was
#       built against the correct API base; loud warning is printed.
#   No apps/web in this checkout AND dist/ already looks valid — auto-skip
#       with the same warning, so external pipelines and partial clones
#       don't break.

set -euo pipefail

SCRIPT_DIR="$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
DESKTOP_DIR="$( dirname "$SCRIPT_DIR" )"
OSS_DIR="$( dirname "$DESKTOP_DIR" )"
DEV_TREE="${PIDASH_DESKTOP_DEV_TREE:-$DESKTOP_DIR/.dev-tree}"
DIST="$DESKTOP_DIR/src-tauri/dist"

HOT_RELOAD="${PIDASH_DESKTOP_HOT_RELOAD:-}"
SKIP="${PIDASH_SKIP_DEV_PREP:-}"

# Conflicting opt-outs — refuse rather than silently pick one. The two flags
# mean different things (hot-reload bypasses dist/ at runtime; skip trusts
# pre-populated dist/) and a build that sets both is almost certainly an
# env-leak bug worth surfacing.
if [[ "$HOT_RELOAD" == "1" && "$SKIP" == "1" ]]; then
    echo "[dev-prep] ERROR: PIDASH_DESKTOP_HOT_RELOAD=1 and PIDASH_SKIP_DEV_PREP=1 are mutually exclusive." >&2
    echo "[dev-prep] Unset whichever you didn't mean to set." >&2
    exit 1
fi

print_bake_info() {
    # Surface what the existing dist/ was last baked against so build logs
    # carry the audit trail even when we skip the rebuild.
    local info="$DIST/bake-info.txt"
    if [[ -f "$info" ]]; then
        echo "[dev-prep] Existing dist/ bake info:"
        sed 's/^/[dev-prep]   /' "$info"
    else
        echo "[dev-prep] WARNING: no $info sidecar — dist/ origin is unknown."
        echo "[dev-prep] WARNING: if the SPA was baked against a different API base, every API call from the shipped binary will go to the wrong host."
    fi
}

if [[ "$HOT_RELOAD" == "1" ]]; then
    echo "[dev-prep] PIDASH_DESKTOP_HOT_RELOAD=1 — skipping bundle build."
    echo "[dev-prep] Expecting a dev server reachable at: ${PI_DASH_URL:-http://localhost:3000}"
    exit 0
fi

if [[ "$SKIP" == "1" ]]; then
    echo "[dev-prep] PIDASH_SKIP_DEV_PREP=1 — skipping bundle build (dist/ assumed pre-populated)."
    print_bake_info
    exit 0
fi

if ! command -v rsync >/dev/null 2>&1; then
    echo "[dev-prep] ERROR: rsync is required for bundled-dev mode but was not found in PATH." >&2
    echo "[dev-prep] Install rsync, or set PIDASH_DESKTOP_HOT_RELOAD=1 to use a dev server instead." >&2
    exit 1
fi

# The web app in this checkout is the source-of-truth for the SPA. If it's
# missing (a partial clone), the only way `cargo tauri build` can still
# produce a usable artifact is to reuse whatever dist/ is already on disk.
# Allow that, but tell the operator what they're getting.
if [[ ! -d "$OSS_DIR/apps/web" ]]; then
    if [[ -f "$DIST/index.html" ]] \
        && [[ $(wc -c < "$DIST/index.html" 2>/dev/null || echo 0) -ge 4096 ]] \
        && [[ -d "$DIST/assets" ]] \
        && [[ -n "$(ls -A "$DIST/assets" 2>/dev/null)" ]]; then
        echo "[dev-prep] No apps/web under $OSS_DIR, but dist/ already looks valid."
        echo "[dev-prep] Reusing existing dist/ (set PIDASH_SKIP_DEV_PREP=1 to silence this in CI)."
        print_bake_info
        exit 0
    fi
    echo "[dev-prep] ERROR: expected the web app at $OSS_DIR/apps/web" >&2
    echo "[dev-prep] Either use a full pi-dash checkout," >&2
    echo "[dev-prep] OR populate dist/ from a known-good frontend bundle and set PIDASH_SKIP_DEV_PREP=1." >&2
    exit 1
fi

# VITE_API_BASE_URL is the API origin baked into the SPA at build time —
# it is NOT the same thing as PI_DASH_URL (the origin main.rs's sign-in
# deep-link handler navigates to). Conflating the two silently ships a
# binary whose SPA hits the wrong host. Require devs to set
# VITE_API_BASE_URL explicitly for non-default targets; only the
# bundled-dev convenience default (a local API, as in apps/web/.env.example)
# applies when unset.
API_BASE="${VITE_API_BASE_URL:-http://localhost:8000}"

# The desktop sign-in card links the user at the web app (ce/components/
# desktop/sign-in-card.tsx reads WEB_URL, i.e. VITE_WEB_BASE_URL). Nothing
# else in a desktop build sets it, and an unset one renders a card with no
# action at all, so default it to the sign-in origin — the same server the
# deep-link hand-off targets. Already in turbo.json globalEnv, so the build
# cache key tracks it.
WEB_BASE="${VITE_WEB_BASE_URL:-${PI_DASH_URL:-}}"

VERIFY_HOOK="${PIDASH_DESKTOP_VERIFY_HOOK:-}"
if [[ -n "$VERIFY_HOOK" && ! -f "$VERIFY_HOOK" ]]; then
    echo "[dev-prep] ERROR: PIDASH_DESKTOP_VERIFY_HOOK is not a file: $VERIFY_HOOK" >&2
    exit 1
fi

echo "[dev-prep] OSS source:  $OSS_DIR"
echo "[dev-prep] Build tree:  $DEV_TREE"
echo "[dev-prep] Dist target: $DIST"
echo "[dev-prep] API base:    $API_BASE  (VITE_API_BASE_URL → baked into SPA)"
echo "[dev-prep] Web base:    ${WEB_BASE:-<unset>}  (VITE_WEB_BASE_URL → sign-in card link)"
echo "[dev-prep] Preparing bundled runner and agent engine"
bash "$SCRIPT_DIR/prepare-agent.sh"
echo "[dev-prep] Sign-in target: ${PI_DASH_URL:-<unset, main.rs default>}  (PI_DASH_URL → main.rs deep-link)"

bash "$SCRIPT_DIR/merge-web-tree.sh" "$DEV_TREE"

cd "$DEV_TREE"

# Honor OSS pi-dash's pinned packageManager (corepack reads it from
# package.json) so we don't end up with a pnpm version that drifts from
# the OSS lockfile's generator.
if command -v corepack >/dev/null 2>&1; then
    corepack enable >/dev/null 2>&1 || true
    corepack prepare --activate >/dev/null 2>&1 || true
fi

echo "[dev-prep] pnpm install"
VITE_API_BASE_URL="$API_BASE" VITE_WEB_BASE_URL="$WEB_BASE" pnpm install --frozen-lockfile

# turbo run build --filter=web follows turbo.json's build.dependsOn ^build
# and builds every workspace package web depends on. pnpm --filter=web
# build alone would skip those and produce either a resolution failure or
# a bundle importing stale dist/ from a prior cache.
#
# --force: .dev-tree/ lives inside this git repo but is gitignored, so
# turbo's git-based input hashing sees each package as just its
# package.json and replays cached output no matter what the overlays
# changed (edition locales into @pi-dash/i18n, desktop-overlay routes
# into web, …). Rebuilding everything costs a minute or two; shipping a
# stale bundle cost a lot more.
echo "[dev-prep] turbo run build --filter=web --force"
VITE_API_BASE_URL="$API_BASE" VITE_WEB_BASE_URL="$WEB_BASE" pnpm exec turbo run build --filter=web --force

CLIENT="$DEV_TREE/apps/web/build/client"
INDEX="$CLIENT/index.html"
if [[ ! -f "$INDEX" ]]; then
    echo "[dev-prep] ERROR: $INDEX was not produced by the web build" >&2
    exit 1
fi

# Same threshold the build.rs guard uses — sanity-check here too so a
# bad build fails in this script (with useful context) rather than later
# in cargo's build.rs panic.
INDEX_SIZE=$(wc -c < "$INDEX")
if (( INDEX_SIZE < 4096 )); then
    echo "[dev-prep] ERROR: $INDEX is only $INDEX_SIZE bytes — looks like a broken build" >&2
    exit 1
fi
if [[ ! -d "$CLIENT/assets" ]] || [[ -z "$(ls -A "$CLIENT/assets" 2>/dev/null)" ]]; then
    echo "[dev-prep] ERROR: $CLIENT/assets is missing or empty" >&2
    exit 1
fi
if ! grep -R -F -q --include='*.html' --include='*.js' --include='*.mjs' --include='*.css' --include='*.json' "$API_BASE" "$CLIENT"; then
    echo "[dev-prep] ERROR: built frontend does not contain VITE_API_BASE_URL=$API_BASE" >&2
    echo "[dev-prep] The SPA may be using a stale constants build or falling back to the webview origin for API calls." >&2
    exit 1
fi
if [[ -n "$VERIFY_HOOK" ]]; then
    echo "[dev-prep] Running verify hook: $VERIFY_HOOK"
    if ! bash "$VERIFY_HOOK" "$CLIENT"; then
        echo "[dev-prep] ERROR: verify hook rejected the built frontend" >&2
        exit 1
    fi
fi

# dist/ replacement. Build into a sibling staging dir, move the live dist/
# aside, then move the staging dir into place. rename(2) cannot replace a
# non-empty directory in one call, so the swap is two renames — but the
# previous bundle survives as $BACKUP for the whole window, and the trap below
# puts it back if we are interrupted (SIGINT, SIGTERM, OOM kill) between them.
# Without that, an interruption left no dist/ at all — including the committed
# index.html placeholder that tauri.conf.json's frontendDist requires — and the
# next cargo build failed until the operator ran `git checkout .../dist`.
STAGING="$DIST.new"
BACKUP="$DIST.prev"

restore_dist() {
    if [[ ! -e "$DIST" && -d "$BACKUP" ]]; then
        mv "$BACKUP" "$DIST" 2>/dev/null || true
        echo "[dev-prep] Restored the previous dist/ after an interrupted swap." >&2
    fi
}
trap restore_dist EXIT INT TERM

echo "[dev-prep] Copying $CLIENT → $STAGING"
rm -rf "$STAGING"
mkdir -p "$STAGING"
cp -a "$CLIENT/." "$STAGING/"

# Record what this dist/ was built against so subsequent SKIP-mode runs
# can surface the bake info in their logs. main.rs doesn't read this; it's
# operator-facing.
{
    echo "api_base=$API_BASE"
    echo "web_base=${WEB_BASE:-<unset>}"
    echo "pi_dash_url=${PI_DASH_URL:-<unset>}"
    echo "oss_sha=${PI_DASH_OSS_SHA:-<unset>}"
    echo "built_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
} > "$STAGING/bake-info.txt"

rm -rf "$BACKUP"
if [[ -e "$DIST" ]]; then
    mv "$DIST" "$BACKUP"
fi
if ! mv "$STAGING" "$DIST"; then
    # Put the previous bundle back rather than leaving no dist/ behind.
    restore_dist
    echo "[dev-prep] ERROR: could not move $STAGING into $DIST" >&2
    exit 1
fi
rm -rf "$BACKUP"

echo "[dev-prep] Done. index.html=${INDEX_SIZE}B, $(ls "$DIST/assets" | wc -l) assets, api_base=$API_BASE."
