// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Estimate reads and point formatting behind the estimate picker
// (parity/specs/issues.md §17, ISS-216). TIME-system values render as
// hours and minutes; points and categories render raw. The active
// system read stays thin: system configuration belongs to the
// estimates epic (§21).

import { queryOptions, useQuery } from "@tanstack/react-query";
import {
  ESTIMATE_SYSTEM_TIME,
  listProjectEstimates,
  selectActiveEstimate,
  type ApiClient,
  type Estimate,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { REFERENCE_STALE_TIME_MS } from "../../../core/query/client.js";
import { issueKeys } from "../api/keys.js";

/**
 * Render one estimate point value. TIME systems store minutes:
 * sub-hour values read as minutes, whole hours without a minutes
 * part, mixed values as hours plus minutes. Anything else — other
 * system types or unparseable TIME values — renders raw.
 */
export function formatEstimatePoint(value: string, systemType: string): string {
  if (systemType !== ESTIMATE_SYSTEM_TIME) return value;
  if (value.trim() === "") return value;
  const minutes = Number(value);
  if (!Number.isInteger(minutes) || minutes < 0) return value;
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  if (rest === 0) return `${hours}h`;
  return `${hours}h ${rest}m`;
}

export function projectEstimatesQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string) {
  return queryOptions({
    queryKey: issueKeys.estimates(workspaceSlug, projectId),
    queryFn: () => listProjectEstimates(client, workspaceSlug, projectId),
    staleTime: REFERENCE_STALE_TIME_MS,
  });
}

/** The project's active estimate system, if one is enabled. */
export function useActiveEstimateSystem(
  workspaceSlug: string,
  projectId: string
): { system: Estimate | undefined; isPending: boolean } {
  const estimates = useQuery(projectEstimatesQueryOptions(getAppClient(), workspaceSlug, projectId));
  return {
    system: estimates.data === undefined ? undefined : selectActiveEstimate(estimates.data),
    isPending: estimates.isPending,
  };
}
