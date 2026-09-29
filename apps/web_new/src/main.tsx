// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { RouterProvider, createRouter } from "@tanstack/react-router";

import { routeTree } from "./routeTree.gen";
import { PIDASH_TARGET } from "./target";
import "./styles/app.css";

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
    <RouterProvider router={router} />
  </StrictMode>
);
