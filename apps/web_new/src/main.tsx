// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { createRouter } from "@tanstack/react-router";

import { createAppClient } from "./core/api/index.js";
import { edition } from "./core/edition/index.js";
import { platform } from "./core/platform/index.js";
import { createQueryClient } from "./core/query/index.js";
import { AppRoot, createAppCachePersistor } from "./features/instance/entry.js";
import { bootstrapIssuesEdition } from "./features/issues/edition/cloudProviders.js";
import { routeTree } from "./routeTree.gen";
import { PIDASH_TARGET } from "./target";
import "./styles/app.css";

// Bootstrap order: platform → HTTP client (also points the session hooks
// at it) → edition seams → query client (+ cache restore) → router.
const apiClient = createAppClient();
// The cloud edition registers its issues providers (team projects, the
// transfer-hop note, work-item types, the detail type badge and the
// provisional workflow rules); the community build registers nothing
// (NEWFRONT-984).
bootstrapIssuesEdition(edition);
const queryClient = createQueryClient();
const persistor = createAppCachePersistor(PIDASH_TARGET, queryClient, platform.storage);
await persistor.restore().catch(() => undefined);

const router = createRouter({
  routeTree,
  context: { queryClient, apiClient },
  defaultPreload: "intent",
  // The console uses the canonical trailing-slash form
  // (parity/specs/admin.md §0.1), which the default would strip when
  // building locations. Preserve keeps authored URLs as-is; member
  // links are slashless, so they are unaffected.
  trailingSlash: "preserve",
});

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}

const rootElement = document.getElementById("root");
if (rootElement === null) {
  throw new Error("Missing #root element");
}

// PIDASH_TARGET selects core/platform at build time. Logged once so smoke
// tests can assert which bundle is being served.
if (import.meta.env.DEV) {
  // eslint-disable-next-line no-console
  console.info(`[web_new] target=${PIDASH_TARGET}`);
}

createRoot(rootElement).render(
  <StrictMode>
    <AppRoot queryClient={queryClient} router={router} />
  </StrictMode>
);
