// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake-state reads behind the intake-state picker
// (parity/specs/issues.md §17/§21, ISS-217). The endpoint answers the
// project's single triage state (or 404 without one); the trigger
// falls back to the first listed state, never the default flag. The
// intake epic owns the triage screens and actions; this only reads.

import { queryOptions, useQuery } from "@tanstack/react-query";
import { getIntakeState, isApiError, type ApiClient, type State } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { REFERENCE_STALE_TIME_MS } from "../../../core/query/client.js";
import { issueKeys } from "../api/keys.js";

/** Trigger fallback: the first intake state, or null when there is none. */
export function selectIntakeFallback(states: State[]): State | null {
  return states[0] ?? null;
}

export function intakeStatesQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string) {
  return queryOptions({
    queryKey: issueKeys.intakeStates(workspaceSlug, projectId),
    queryFn: async (): Promise<State[]> => {
      try {
        return [await getIntakeState(client, workspaceSlug, projectId)];
      } catch (error) {
        if (isApiError(error) && error.status === 404) return [];
        throw error;
      }
    },
    staleTime: REFERENCE_STALE_TIME_MS,
  });
}

/** The project's intake states: the single triage state, or empty. */
export function useIntakeStates(
  workspaceSlug: string,
  projectId: string
): { states: State[] | undefined; isPending: boolean } {
  const query = useQuery(intakeStatesQueryOptions(getAppClient(), workspaceSlug, projectId));
  return { states: query.data, isPending: query.isPending };
}
