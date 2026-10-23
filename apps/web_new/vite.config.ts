// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import tailwindcss from "@tailwindcss/vite";
import { TanStackRouterVite } from "@tanstack/router-plugin/vite";
import react from "@vitejs/plugin-react";
import { defineConfig, type ProxyOptions } from "vite";

import { hostedBaseUrlFromEnv } from "./src/core/platform/hostedBaseUrl.js";
import { publicSiteOriginFromEnv } from "./src/core/platform/publicSiteOrigin.js";
import { resolveEditionModule } from "./tools/edition-module.mjs";

const target = process.env.PIDASH_TARGET === "desktop" ? "desktop" : "web";
const here = dirname(fileURLToPath(import.meta.url));
// Local Django for `pnpm dev`. Overridden per shell with e.g.
// `export PIDASH_API_ORIGIN="http://127.0.0.1:8123"` on its own line.
const apiOrigin = process.env.PIDASH_API_ORIGIN ?? "http://localhost:8000";
// Hosted web app origin for shareable links (DESK-019) and the desktop
// unavailable-sign-in card (AUTH-023). The desktop pipeline bakes
// VITE_WEB_BASE_URL; PIDASH_WEB_BASE_URL is accepted too. Builds without
// either leave the define null so links refuse rather than guess an origin.
const webBaseUrl = hostedBaseUrlFromEnv(process.env);
// API origin baked into the client. Empty (the default) keeps calls on the
// serving origin and makes the add-runner command fall back to it too.
const apiBaseUrl = process.env.VITE_API_BASE_URL ?? "";
// Origin for crawlable marketing metadata (CLOUD-005). The prerender step
// reads the same variable, so a deployment's canonical/og:url/sitemap all
// name one address before and after hydration.
const publicSiteOrigin = publicSiteOriginFromEnv(process.env);
// Edition swap (maintainer decision 2026-10-18): PIDASH_EDITION_MODULE
// points @pidash/edition at the private cloud module; unset keeps oss.ts.
const { modulePath: editionModule, fsAllow: editionFsAllow } = resolveEditionModule(process.env, here);

// Static SPA only (D9). One bundle per target; the desktop bundle is loaded
// by the Tauri shell. No file swapping: platform differences live behind
// core/platform (F-04), selected here at build time.

/**
 * Point proxied redirects back at the dev server. Native auth views answer
 * with absolute locations at the backend's configured web base; the client
 * follows them with fetch, which fails cross-origin. Rewriting the host to
 * the incoming request's host restores the single-origin production shape.
 */
const rewriteRedirectToDevOrigin: NonNullable<ProxyOptions["configure"]> = (proxy) => {
  proxy.on("proxyRes", (proxyRes, req) => {
    const location = proxyRes.headers["location"];
    if (typeof location !== "string" || location.length === 0) return;
    let target: URL;
    try {
      target = new URL(location);
    } catch {
      return;
    }
    // Only backend-local redirects (the app itself on loopback); provider
    // hand-offs to external hosts pass through untouched.
    if (!["localhost", "127.0.0.1", "[::1]"].includes(target.hostname)) return;
    const host = req.headers?.["host"];
    if (typeof host !== "string" || host.length === 0 || target.host === host) return;
    target.host = host;
    target.protocol = "http:";
    proxyRes.headers["location"] = target.toString();
  });
};
export default defineConfig(({ mode }) => {
  const buildTarget = mode === "desktop" || target === "desktop" ? "desktop" : "web";
  return {
    define: {
      __PIDASH_TARGET__: JSON.stringify(buildTarget),
      __PIDASH_WEB_BASE_URL__: JSON.stringify(webBaseUrl),
      __PIDASH_API_BASE_URL__: JSON.stringify(apiBaseUrl),
      __PIDASH_PUBLIC_SITE_ORIGIN__: JSON.stringify(publicSiteOrigin),
    },
    outDir: `dist/${buildTarget}`,
    resolve: {
      alias: [
        // Resolve the workspace client to source so dev/test/typecheck
        // never depend on a prior package build; CI builds it anyway.
        { find: "@pidash/api-client", replacement: resolve(here, "../../packages/api-client/src/index.ts") },
        // Same for the design system now that screens render kit components.
        // Exact match only: the kit CSS entries must keep resolving to the
        // built package (see styles/app.css).
        { find: /^@pidash\/kit$/, replacement: resolve(here, "../../packages/kit/src/index.ts") },
        // Single edition swap point: PIDASH_EDITION_MODULE or oss.ts.
        { find: "@pidash/edition", replacement: editionModule },
        // Public sources for the edition module: the private cloud module
        // imports its slot components from public web_new through here
        // (maintainer decision 2026-10-18), so no checkout layout is
        // assumed. App code never uses this; it imports relatively.
        { find: /^@web_new\/(.+)$/, replacement: `${resolve(here, "src")}/$1` },
        // Build-time platform selection: only one implementation is ever
        // bundled, so the web bundle cannot contain desktop code.
        {
          find: "@pidash/platform-target",
          replacement:
            buildTarget === "desktop"
              ? resolve(here, "src/core/platform/tauri.ts")
              : resolve(here, "src/core/platform/web.ts"),
        },
      ],
    },
    plugins: [
      TanStackRouterVite({ target: "react", autoCodeSplitting: true }),
      react({
        babel: {
          plugins: [["babel-plugin-react-compiler", {}]],
        },
      }),
      tailwindcss(),
    ],
    server: {
      port: 3010,
      strictPort: true,
      // An edition module outside the repo (the private checkout) needs
      // an explicit serve allowance; in-repo modules need none.
      ...(editionFsAllow === null ? {} : { fs: { allow: editionFsAllow } }),
      proxy: {
        // Keep the browser Host: Django compares it against Origin for
        // native form views, and the session cookie stays host-only.
        // Rewrite redirect targets to the dev origin: the backend answers
        // native form POSTs with absolute locations at its configured web
        // base (the deployed origin, or the parity oracle in tests), and a
        // split-origin dev server would otherwise follow them cross-origin
        // into a CORS failure. Production serves app and API on one origin,
        // so this only restores that topology for `pnpm dev`.
        // The client owns /auth/device (device approval page): bypass the
        // proxy for it so the SPA serves the route (production Caddy routes
        // the same prefix to the app, not the API).
        "/auth": {
          target: apiOrigin,
          changeOrigin: false,
          configure: rewriteRedirectToDevOrigin,
          bypass: (req) => {
            // Serve the app shell for client-owned paths (the browser URL
            // is untouched, so client routing still sees /auth/device).
            const pathname = (req.url ?? "").split("?", 1)[0];
            if (pathname === "/auth/device" || pathname === "/auth/device/") return "/index.html";
            return undefined;
          },
        },
        "/api": { target: apiOrigin, changeOrigin: false, configure: rewriteRedirectToDevOrigin },
        // Object storage in parity runs (NEWFRONT-586; production-shape
        // parity with the stack Caddyfile's NEWFRONT-143 bucket route): the
        // API presigns bucket URLs against the request host, which is this
        // dev server through the /api proxy above, so serve the bucket path
        // from the slot's minio. PARITY_MINIO_PORT is printed by
        // parity-up.sh; unset leaves local dev untouched. changeOrigin
        // stays false: the presigned signature covers the addressed host.
        ...(process.env.PARITY_MINIO_PORT === undefined
          ? {}
          : {
              "/parity19-assets": {
                target: `http://localhost:${process.env.PARITY_MINIO_PORT}`,
                changeOrigin: false,
              },
            }),
      },
    },
    build: {
      outDir: `dist/${buildTarget}`,
      emptyOutDir: true,
      sourcemap: false,
      chunkSizeWarningLimit: 200,
      rollupOptions: {
        treeshake: {
          // The projects list-display store and its math are side-effect-free
          // (the sign-out registration lives in the list view's effect), so
          // lazy-only use must not pin them into the initial bundle through
          // the feature barrel (NEWFRONT-523, size gate). The issues row
          // highlight and mutation loader are likewise pure context plus
          // components, lazy-only behind the layout root (NEWFRONT-614).
          // The shared editor read surface and lazy editor are pure too
          // (the lazy call carries a PURE annotation): without the listing
          // the barrel pins the sanitizer plus DOMPurify into the initial
          // bundle (NEWFRONT-639, size gate). The sub-issue widget renders
          // only under the detail route, but its section reaches the store
          // that registers a sign-out reset at load; without the listing
          // the barrel drags the whole widget into the initial bundle
          // (NEWFRONT-647, size gate). The store keeps its registration:
          // it only loads with the widget, and a store nobody loaded has
          // no state to reset. The saved-view layout body is lazy-only: it
          // builds its per-layout React.lazy loaders at module scope, which
          // otherwise pins them (and the layouts behind them) into the
          // initial chunk through the issues barrel (NEWFRONT-869). The
          // attachments widget is lazy-only too: its module-scope
          // forwardRef call otherwise pins the widget, its reads and the
          // pickers behind them into the initial chunk through the same
          // barrel (NEWFRONT-870, perf budgets). The modules contract is
          // lazy-only as well: its schemas' module-scope initializers
          // otherwise pin the unused contract into the initial chunk
          // through the api-client barrel (NEWFRONT-870, size gate).
          // The scheduler calendar modules are pure
          // views and helpers reached only through the schedulers barrel;
          // the calendar route loads its body lazily, so without the
          // listing the barrel pins the grids, rail and panel into the
          // initial chunk (NEWFRONT-593, size gate).
          // Every other module keeps the default: a blanket false reshapes
          // the TanStack route splits the size gate keys on.
          moduleSideEffects: (id) =>
            id.includes("features/projects/store/list-display.") ||
            id.includes("features/projects/lib/list-display.") ||
            id.includes("features/issues/freshness/highlight.") ||
            id.includes("features/issues/loaders/loaderContext.") ||
            id.includes("shared/editor/sanitize.") ||
            id.includes("shared/editor/RichTextEditor.") ||
            id.includes("features/issues/subIssues/SubIssuesWidget.") ||
            id.includes("features/issues/subIssues/SubIssueSection.") ||
            id.includes("features/issues/subIssues/SubIssueRow.") ||
            id.includes("features/issues/subIssues/SubIssueProperties.") ||
            id.includes("features/issues/subIssues/SubIssueFilterMenu.") ||
            id.includes("features/issues/subIssues/SubIssueDisplayMenu.") ||
            id.includes("features/issues/subIssues/AddExistingDialog.") ||
            id.includes("features/issues/layouts/ViewLayoutBody.") ||
            id.includes("features/issues/detail/widgets/attachments/AttachmentsWidget.") ||
            id.includes("packages/api-client/src/contracts/modules.") ||
            id.includes("features/schedulers/FiringCalendar.") ||
            id.includes("features/schedulers/CalendarWeekView.") ||
            id.includes("features/schedulers/CalendarMonthView.") ||
            id.includes("features/schedulers/VisibilityRail.") ||
            id.includes("features/schedulers/OccurrencePanel.") ||
            id.includes("features/schedulers/calendarModel.") ||
            id.includes("features/schedulers/calendarText.") ||
            id.includes("features/schedulers/occurrences.") ||
            id.includes("features/schedulers/visibility.")
              ? false
              : true,
        },
      },
      // The size gate (.size-limit.mjs) resolves the initial route chain
      // from this manifest, so chunk hashes can rotate without touching
      // the budget config (NEWFRONT-190).
      manifest: true,
    },
  };
});
