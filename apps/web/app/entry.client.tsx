/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { startTransition, StrictMode } from "react";
import { hydrateRoot } from "react-dom/client";
import { HydratedRouter } from "react-router/dom";
// utils
import { reloadOnceForStaleChunk } from "@/app/error/chunk-reload";

// Vite fires this when a dynamic import fails to load — typically a stale-deploy
// chunk that the new build no longer serves. Recover by reloading once (guarded
// against loops) and preventDefault so it never bubbles to the error boundary.
window.addEventListener("vite:preloadError", (event) => {
  if (reloadOnceForStaleChunk()) event.preventDefault();
});

startTransition(() => {
  hydrateRoot(
    document,
    <StrictMode>
      <HydratedRouter />
    </StrictMode>
  );
});
