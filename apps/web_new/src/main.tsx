// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { RouterProvider, createRouter } from "@tanstack/react-router";
import { QueryClientProvider } from "@tanstack/react-query";
import { createClient } from "@pidash/api-client";

import { routeTree } from "./routeTree.gen";
import { PIDASH_TARGET } from "./target";
import { platform, toApiTransport } from "./core/platform/index.js";
import { edition } from "./core/edition/index.js";
import {
  createCachePersistor,
  createQueryClient,
  persistEverything,
  persistReferenceOnly,
} from "./core/query/index.js";
import { createSessionMiddleware, setSessionClient } from "./core/session/index.js";
import "./styles/app.css";

// Bootstrap order (Architecture): platform → edition → queryClient → router.
const queryClient = createQueryClient();
const apiClient = createClient({
  baseUrl: "",
  transport: toApiTransport(platform),
  middleware: [...(edition.api ?? []), createSessionMiddleware()],
});
setSessionClient(apiClient);
const persistor = createCachePersistor(queryClient, platform.storage, {
  shouldPersist: platform.kind === "desktop" ? persistEverything : persistReferenceOnly,
});
void persistor.restore();

const router = createRouter({ routeTree });

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}

const rootElement = document.getElementById("root");
if (rootElement === null) {
  throw new Error("Missing #root element");
}

// PIDASH_TARGET selects core/platform at build time (F-04 expands this into
// the web/desktop platform modules). Logged once so smoke tests can assert
// which bundle is being served.
if (import.meta.env.DEV) {
  // eslint-disable-next-line no-console
  console.info(`[web_new] target=${PIDASH_TARGET}`);
}

createRoot(rootElement).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  </StrictMode>
);
