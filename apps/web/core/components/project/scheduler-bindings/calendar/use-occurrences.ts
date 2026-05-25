/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import useSWR from "swr";
import type { ISchedulerOccurrence, ISchedulerOccurrenceResponse } from "@pi-dash/services";
import { SchedulerService } from "@pi-dash/services";

const schedulerService = new SchedulerService();

type Params = {
  workspaceSlug: string;
  projectId: string;
  /** UTC ISO datetime (inclusive). */
  from: string;
  /** UTC ISO datetime (inclusive). */
  to: string;
};

type UseOccurrencesResult = {
  occurrences: ISchedulerOccurrence[];
  hasMore: boolean;
  nextWindowStart: string | null;
  isLoading: boolean;
  error: unknown;
  mutate: () => Promise<unknown>;
};

/**
 * SWR-backed hook for the project's calendar occurrences endpoint.
 *
 * Refetches whenever the visible date window changes. The endpoint caps
 * the response at 5000 occurrences and returns ``has_more`` + a
 * ``next_window_start`` cursor — the calendar surfaces that as a "narrow
 * the date range" hint rather than auto-paging.
 */
export function useSchedulerOccurrences({ workspaceSlug, projectId, from, to }: Params): UseOccurrencesResult {
  const key =
    workspaceSlug && projectId && from && to ? ["scheduler-occurrences", workspaceSlug, projectId, from, to] : null;
  const { data, error, isLoading, mutate } = useSWR<ISchedulerOccurrenceResponse>(
    key,
    () => schedulerService.listOccurrences(workspaceSlug, projectId, { from, to }),
    {
      // Stale-while-revalidate is fine here; the endpoint itself caches.
      revalidateOnFocus: false,
    }
  );

  return {
    occurrences: data?.occurrences ?? [],
    hasMore: data?.has_more ?? false,
    nextWindowStart: data?.next_window_start ?? null,
    isLoading,
    error,
    mutate,
  };
}
