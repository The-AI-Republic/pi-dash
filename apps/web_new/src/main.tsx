// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { RouterProvider, createRouter } from "@tanstack/react-router";

import { createAppClient } from "./core/api/index.js";
import { platform } from "./core/platform/index.js";
import { createCachePersistor, createQueryClient, persistReferenceOnly } from "./core/query/index.js";
import { routeTree } from "./routeTree.gen";
import { PIDASH_TARGET } from "./target";
import "./styles/app.css";

// Bootstrap order: platform → HTTP client (also points the session hooks
// at it) → query client (+ cache restore) → router.
const apiClient = createAppClient();
const queryClient = createQueryClient();
const persistor = createCachePersistor(queryClient, platform.storage, {
  shouldPersist: PIDASH_TARGET === "desktop" ? () => true : persistReferenceOnly,
});
await persistor.restore().catch(() => undefined);

const router = createRouter({
  routeTree,
  context: { queryClient, apiClient },
  defaultPreload: "intent",
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
    <RouterProvider router={router} />
  </StrictMode>
);
