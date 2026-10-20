// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Plan snapshot reads (parity/specs/cloud.md §8, CLOUD-038). The badge
// reads the snapshot on mount and again whenever the window regains
// focus; a matching answer reuses the previous object so no subscriber
// re-renders for a refresh that changed nothing visible. A failed read
// keeps the last known snapshot; the next focus retries it.

import { queryOptions, useQuery } from "@tanstack/react-query";
import { getCloudBillingPlan, type ApiClient } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { isCloudPlan, samePlan } from "../lib/plan.js";
import { cloudBillingKeys } from "./keys.js";

export function cloudPlanQueryOptions(client: ApiClient) {
  return queryOptions({
    queryKey: cloudBillingKeys.plan,
    queryFn: () => getCloudBillingPlan(client),
    // Always stale: regaining focus is the badge's refresh trigger, and a
    // matching answer is deduped below instead of by freshness.
    staleTime: 0,
    refetchOnWindowFocus: true,
    // A plan snapshot is one small read; automatic retries would only
    // delay the next focus retry the badge already offers.
    retry: false,
    // A matching answer reuses the previous object, so subscribers see no
    // change and the badge does not re-render for a refresh that changed
    // nothing visible.
    structuralSharing: (previous: unknown, next: unknown): unknown =>
      isCloudPlan(previous) && isCloudPlan(next) && samePlan(previous, next) ? previous : next,
  });
}

/** The signed-in account's plan snapshot for the workspace badge. */
export function useCloudPlan(enabled: boolean) {
  return useQuery({ ...cloudPlanQueryOptions(getAppClient()), enabled });
}
