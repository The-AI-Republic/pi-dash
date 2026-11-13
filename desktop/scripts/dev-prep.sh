#!/usr/bin/env bash
# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Populates desktop/src-tauri/dist/ with the apps/web_new desktop bundle for
# bundled-dev mode (the default of `cargo tauri dev`) and for local
# `cargo tauri build`. Invoked automatically via tauri.conf.json's
# beforeDevCommand / beforeBuildCommand.
#
# The desktop shell ships one frontend: apps/web_new built with
# PIDASH_TARGET=desktop. The old apps/web tree and desktop-overlay/ are no
# longer part of the desktop bundle (NEWFRONT-92/M-01).
#
# PIDASH_DESKTOP_VERIFY_HOOK, when set, is run with the built client
# directory as its only argument after the generic checks below pass, so an
# edition can assert things about its own bundle.
#
# Skip-paths:
#   PIDASH_DESKTOP_HOT_RELOAD=1 — webview opens at PI_DASH_URL directly,
#       dist/ is unused.
#   PIDASH_SKIP_DEV_PREP=1 — caller has already populated dist/ (CI, or
#       a manually-managed local artifact). Cannot verify the SPA was
#       built against the correct API base; loud warning is printed.
#
# pnpm and the Vite/turbo caches stay warm across runs.

set -euo pipefail

SCRIPT_DIR="$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
DESKTOP_DIR="$( dirname "$SCRIPT_DIR" )"
OSS_DIR="$( dirname "$DESKTOP_DIR" )"
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
        echo "[dev-prep] WARNING: if the SPA was baked against different API/web bases, API calls or copied links from the shipped binary can target the wrong host."
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

# VITE_API_BASE_URL is the API origin the SPA learns at build time —
# it is NOT the same thing as PI_DASH_URL (the origin main.rs's sign-in
# deep-link handler navigates to). Conflating the two silently ships a
# binary whose SPA hits the wrong host. Require devs to set
# VITE_API_BASE_URL explicitly for non-default targets; only the
# bundled-dev convenience default (a local API) applies when unset.
API_BASE="${VITE_API_BASE_URL:-http://localhost:8000}"

# The public web origin baked into the SPA (VITE_WEB_BASE_URL → the
# runtime's hosted base). web_new resolves shareable links and the
# desktop browser hand-off against it, and refuses rather than guessing an
# origin — bundled pages run on a Tauri origin, so anything resolved
# against window.location.origin would come out as tauri://localhost. It
# also has to be baked for build.rs's release guard. Default it to the
# sign-in origin, the same server the deep-link hand-off targets, and fall
# back to main.rs's own bundled-dev default for PI_DASH_URL so the value is
# never empty.
WEB_BASE="${VITE_WEB_BASE_URL:-${PI_DASH_URL:-http://localhost:8000}}"

VERIFY_HOOK="${PIDASH_DESKTOP_VERIFY_HOOK:-}"
if [[ -n "$VERIFY_HOOK" && ! -f "$VERIFY_HOOK" ]]; then
    echo "[dev-prep] ERROR: PIDASH_DESKTOP_VERIFY_HOOK is not a file: $VERIFY_HOOK" >&2
    exit 1
fi

# The legacy frontend switch is retired (NEWFRONT-92/M-01): apps/web_new is
# the only desktop frontend, so an environment that still asks for the old
# overlay-merged tree would otherwise build something this script no longer
# produces. Refuse loudly instead of silently shipping web_new.
case "${PIDASH_DESKTOP_WEB:-}" in
    ""|web_new) ;;
    *)
        echo "[dev-prep] ERROR: PIDASH_DESKTOP_WEB=${PIDASH_DESKTOP_WEB} is no longer supported." >&2
        echo "[dev-prep] The desktop bundles apps/web_new; unset PIDASH_DESKTOP_WEB (or set it to web_new)." >&2
        exit 1
        ;;
esac


if [[ ! -d "$OSS_DIR/apps/web_new" ]]; then
    echo "[dev-prep] ERROR: expected the desktop frontend at $OSS_DIR/apps/web_new" >&2
    echo "[dev-prep] Use a full pi-dash checkout, OR populate dist/ from a known-good" >&2
    echo "[dev-prep] bundle and set PIDASH_SKIP_DEV_PREP=1." >&2
    exit 1
fi

echo "[dev-prep] OSS source:  $OSS_DIR"
echo "[dev-prep] Dist target: $DIST"
echo "[dev-prep] API base:    $API_BASE  (VITE_API_BASE_URL → baked into SPA)"
echo "[dev-prep] Web base:    ${WEB_BASE:-<unset>}  (VITE_WEB_BASE_URL → baked into SPA)"
echo "[dev-prep] Frontend:    web_new (PIDASH_TARGET=desktop)"
echo "[dev-prep] Preparing bundled CLI"
bash "$SCRIPT_DIR/prepare-agent.sh"
echo "[dev-prep] Sign-in target: ${PI_DASH_URL:-<unset, main.rs default>}  (PI_DASH_URL → main.rs deep-link)"

if command -v corepack >/dev/null 2>&1; then
    corepack enable >/dev/null 2>&1 || true
    corepack prepare --activate >/dev/null 2>&1 || true
fi

echo "[dev-prep] pnpm install"
( cd "$OSS_DIR" && VITE_API_BASE_URL="$API_BASE" VITE_WEB_BASE_URL="$WEB_BASE" pnpm install --frozen-lockfile )

# web_new imports "@pidash/kit/kit.css" (apps/web_new/src/styles/app.css)
# and the package exports it from ./dist/kit.css, which is gitignored.
# `pnpm --filter web_new build:desktop` alone does not build workspace
# dependencies, so a clean checkout fails with
# "Package path ./kit.css is exported from package .../@pidash/kit,
# but no valid target file was found". Build both workspace deps first,
# mirroring .github/workflows/web-new.yml (builds kit + api-client
# before web_new) (PIDESKAPP-185).
echo "[dev-prep] pnpm --filter @pidash/kit build"
( cd "$OSS_DIR" && VITE_API_BASE_URL="$API_BASE" VITE_WEB_BASE_URL="$WEB_BASE" pnpm --filter @pidash/kit build )

echo "[dev-prep] pnpm --filter @pidash/api-client build"
( cd "$OSS_DIR" && VITE_API_BASE_URL="$API_BASE" VITE_WEB_BASE_URL="$WEB_BASE" pnpm --filter @pidash/api-client build )

echo "[dev-prep] pnpm --filter web_new build:desktop"
( cd "$OSS_DIR" && VITE_API_BASE_URL="$API_BASE" VITE_WEB_BASE_URL="$WEB_BASE" PIDASH_TARGET=desktop pnpm --filter web_new build:desktop )

CLIENT="$OSS_DIR/apps/web_new/dist/desktop"
INDEX="$CLIENT/index.html"
if [[ ! -f "$INDEX" ]]; then
    echo "[dev-prep] ERROR: $INDEX was not produced by the web_new desktop build" >&2
    exit 1
fi

# web_new ships a tiny SPA shell (index.html only bootstraps assets/), so a
# byte threshold cannot tell a good build from a broken one. Require the
# shell to reference its asset bundle; the assets/ and desktop-marker checks
# below complete the picture.
if ! grep -F -q 'assets/' "$INDEX"; then
    echo "[dev-prep] ERROR: $INDEX does not reference its assets/ bundle — looks like a broken build" >&2
    exit 1
fi
if [[ ! -d "$CLIENT/assets" ]] || [[ -z "$(ls -A "$CLIENT/assets" 2>/dev/null)" ]]; then
    echo "[dev-prep] ERROR: $CLIENT/assets is missing or empty" >&2
    exit 1
fi
# Prove the bundle is the desktop target: only core/platform/tauri.ts can
# emit this marker, and the build-time alias excludes it from the web
# bundle (F-04). A web bundle in the shell would still render, with no
# native transport, no cookie jar and no browser hand-off.
if ! grep -R -F -q --include='*.html' --include='*.js' --include='*.mjs' --include='*.css' --include='*.json' "desktop_api_request" "$CLIENT"; then
    echo "[dev-prep] ERROR: $CLIENT has no desktop platform marker" >&2
    echo "[dev-prep] The bundle was built without PIDASH_TARGET=desktop; refusing to ship a web bundle in the desktop shell." >&2
    exit 1
fi
# build.rs's release guard checks the same two values from the bundle, so
# fail here with useful context instead of later inside cargo.
if ! grep -R -F -q --include='*.html' --include='*.js' --include='*.mjs' --include='*.css' --include='*.json' "$API_BASE" "$CLIENT"; then
    echo "[dev-prep] ERROR: built frontend does not contain VITE_API_BASE_URL=$API_BASE" >&2
    exit 1
fi
if ! grep -R -F -q --include='*.html' --include='*.js' --include='*.mjs' --include='*.css' --include='*.json' "$WEB_BASE" "$CLIENT"; then
    echo "[dev-prep] ERROR: built frontend does not contain VITE_WEB_BASE_URL=$WEB_BASE" >&2
    echo "[dev-prep] Every \"Copy link\" and the unmigrated-screen browser hand-off would fail: addresses resolve against this origin." >&2
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
# previous bundle survives as $BACKUP for the whole window, and the trap
# below puts it back if we are interrupted (SIGINT, SIGTERM, OOM kill)
# between them. Without that, an interruption left no dist/ at all —
# including the committed index.html placeholder that tauri.conf.json's
# frontendDist requires — and the next cargo build failed until the
# operator ran `git checkout .../dist`.
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

# Record what this dist/ was built against so subsequent SKIP-mode runs can
# surface it, and so build.rs can hand api_base to main.rs for its startup
# mismatch warning.
{
    echo "frontend=web_new"
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
    restore_dist
    echo "[dev-prep] ERROR: could not move $STAGING into $DIST" >&2
    exit 1
fi
rm -rf "$BACKUP"

INDEX_SIZE=$(wc -c < "$DIST/index.html")
echo "[dev-prep] Done. index.html=${INDEX_SIZE}B, $(ls "$DIST/assets" | wc -l) assets, api_base=$API_BASE, web_base=$WEB_BASE, frontend=web_new."
