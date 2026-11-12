// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Upkeep reads and writes (parity/specs/profile.md §11, PROF-024). The
// read is the member's own switches over the instance's scheduled jobs.
// Writes patch the cache before the request and roll back the exact
// previous payload when it fails, then re-read so the server stays
// authoritative.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { getUpkeepSettings, setUpkeepEnabled, setUpkeepJobEnabled, type UpkeepSettings } from "@pidash/api-client";

import { getAppClient } from "../../core/api/client.js";
import { REFERENCE_STALE_TIME_MS } from "../../core/query/client.js";
import { applyOptimisticUpdate, type Rollback } from "../../core/query/optimistic.js";
import { notifyError } from "../../shared/shell/toaster.js";
import { UPKEEP_FAILED_TITLE } from "./lib/copy.js";
import { upkeepFailureMessage, withJobToggled, withMasterPaused } from "./lib/rules.js";

export const upkeepKeys = {
  all: ["profile-upkeep"] as const,
  settings: ["profile-upkeep", "settings"] as const,
};

/** The member's upkeep switches; never polled, re-read on mount only. */
export function useUpkeepSettings() {
  return useQuery({
    queryKey: upkeepKeys.settings,
    queryFn: () => getUpkeepSettings(getAppClient()),
    staleTime: REFERENCE_STALE_TIME_MS,
    refetchOnWindowFocus: false,
  });
}

function reportFailure(error: unknown): void {
  notifyError(UPKEEP_FAILED_TITLE, upkeepFailureMessage(error));
}

/** Flip the master pause; individual jobs keep their own switches. */
export function useSetUpkeepPaused() {
  const queryClient = useQueryClient();
  return useMutation<UpkeepSettings, Error, boolean, Rollback>({
    mutationFn: (paused) => setUpkeepEnabled(getAppClient(), !paused),
    onMutate: (paused) => {
      const rollback = applyOptimisticUpdate<UpkeepSettings>(queryClient, upkeepKeys.settings, (previous) =>
        withMasterPaused(previous, paused)
      );
      return rollback;
    },
    onSuccess: (settings) => {
      queryClient.setQueryData(upkeepKeys.settings, settings);
    },
    onError: (error, _paused, rollback) => {
      rollback?.();
      reportFailure(error);
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: upkeepKeys.settings }),
  });
}

export interface UpkeepJobToggleVars {
  slug: string;
  enabled: boolean;
}

/** Flip one scheduled job. */
export function useSetUpkeepJobEnabled() {
  const queryClient = useQueryClient();
  return useMutation<UpkeepSettings, Error, UpkeepJobToggleVars, Rollback>({
    mutationFn: ({ slug, enabled }) => setUpkeepJobEnabled(getAppClient(), slug, enabled),
    onMutate: ({ slug, enabled }) => {
      const rollback = applyOptimisticUpdate<UpkeepSettings>(queryClient, upkeepKeys.settings, (previous) =>
        withJobToggled(previous, slug, enabled)
      );
      return rollback;
    },
    onSuccess: (settings) => {
      queryClient.setQueryData(upkeepKeys.settings, settings);
    },
    onError: (error, _vars, rollback) => {
      rollback?.();
      reportFailure(error);
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: upkeepKeys.settings }),
  });
}
