// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake route preload (parity/specs/intake.md INT-001–005, 012, 031).
// The route file is eager (the router imports every route), so the screen
// and its reads live here, on the visit's own chunk. The route starts the
// chunk with its module and awaits the same request in `beforeLoad`, so
// the first commit is the screen with its data (NEWFRONT-869).

import type { ApiClient } from "@pidash/api-client";
import type { QueryClient } from "@tanstack/react-query";

import { projectQueryOptions } from "../../auth/index.js";
import { prefetchIntakeList } from "../api/queries.js";
import { parseIntakeSearch, type IntakeSearch } from "../lib/search.js";

/** The route params the intake screen and its reads take. */
export interface IntakeRouteParams {
  ws: string;
  projectId: string;
}

/**
 * Warm the list the screen is about to ask for. The gate is the screen's
 * own job (it reads the project row), so the prefetch reads the same row
 * once — the screen reuses it — and skips the list while the feature is
 * off. A failed row read leaves the decision to the screen; this never
 * turns a visit into a load error.
 */
export async function preloadIntakeRoute(
  queryClient: QueryClient,
  apiClient: ApiClient,
  params: IntakeRouteParams,
  search: Record<string, unknown>
): Promise<void> {
  const parsed: IntakeSearch = parseIntakeSearch(search);
  const project = await queryClient
    .ensureQueryData(projectQueryOptions(apiClient, params.ws, params.projectId))
    .catch(() => undefined);
  if (project === undefined || !project.inbox_view) return;
  await prefetchIntakeList(queryClient, apiClient, params.ws, params.projectId, {
    tab: parsed.currentTab,
  });
}
