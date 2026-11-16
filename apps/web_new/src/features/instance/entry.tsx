// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// App entry composition (parity/specs/workspace-settings.md §34-35,
// WSET-034/035). The DOM root lives in main.tsx; everything that decides
// behavior is here so the startup gate and the desktop cache policy are
// unit-tested rather than reachable only from the browser entry: which
// queries the desktop persists, and the provider/gate/router stack whose
// order is the guarantee that maintenance replaces every route.

import type { QueryClient } from "@tanstack/react-query";
import { QueryClientProvider } from "@tanstack/react-query";
import type { AnyRouter } from "@tanstack/react-router";
import { RouterProvider } from "@tanstack/react-router";
import * as React from "react";

import type { KeyValueStore } from "../../core/platform/types.js";
import {
  createCachePersistor,
  persistEverything,
  persistReferenceOnly,
  type CachePersistor,
} from "../../core/query/index.js";
import type { PidashTarget } from "../../target.js";
import { InstanceAppGate } from "./InstanceAppGate.js";
import { persistAppCache } from "./queries.js";

/**
 * The cache filter for one build target. Desktop keeps the whole cache so a
 * warm launch paints at once, but never the one-shot startup answer: a
 * restored answer would skip the read and could strand the app on a stale
 * setup or maintenance branch. Web persists reference data only.
 */
export function appCacheShouldPersist(target: PidashTarget): (queryKey: readonly unknown[]) => boolean {
  if (target !== "desktop") return persistReferenceOnly;
  return (queryKey) => persistEverything() && persistAppCache(queryKey);
}

/** Build the launch persistor for a target and restore nothing yet. */
export function createAppCachePersistor(
  target: PidashTarget,
  queryClient: QueryClient,
  storage: KeyValueStore
): CachePersistor {
  return createCachePersistor(queryClient, storage, {
    shouldPersist: appCacheShouldPersist(target),
  });
}

export interface AppRootProps {
  queryClient: QueryClient;
  router: AnyRouter;
}

/**
 * The whole application stack. The gate sits above the router, so a
 * maintenance or not-ready answer replaces every route it owns rather than
 * painting workspace chrome around a message.
 */
export function AppRoot({ queryClient, router }: AppRootProps): React.ReactElement {
  return (
    <QueryClientProvider client={queryClient}>
      <InstanceAppGate>
        <RouterProvider router={router} />
      </InstanceAppGate>
    </QueryClientProvider>
  );
}
