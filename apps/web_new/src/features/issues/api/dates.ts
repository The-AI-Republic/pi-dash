// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Issue date writes (parity/specs/issues.md §17, ISS-214/215). One PATCH
// carrying both start and target dates — clearing sends both null in
// that single update (never the two concurrent single-field writes of
// bug NEWFRONT-150). Applies optimistically across list and detail
// caches, rolls back with a toast on failure.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { updateIssue } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { rollbackSnapshots, snapshotQueries } from "../../../core/query/optimistic.js";
import { notifyError } from "../../../shared/shell/index.js";
import { issueKeys } from "./keys.js";

/** Failure title when the date write does not land. */
export const ISSUE_DATES_FAILED_TITLE = "Couldn't save the dates";
const ISSUE_DATES_FAILED_BODY = "The issue dates are unchanged — check your connection and try again.";

export interface IssueDatesInput {
  startDate: string | null;
  endDate: string | null;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function withRowDates(row: unknown, issueId: string, dates: IssueDatesInput): unknown {
  if (!isRecord(row) || row.id !== issueId) return row;
  return { ...row, start_date: dates.startDate, target_date: dates.endDate };
}

function withBucketDates(bucket: unknown, issueId: string, dates: IssueDatesInput): unknown {
  if (!isRecord(bucket) || !Array.isArray(bucket.results)) return bucket;
  return { ...bucket, results: bucket.results.map((row) => withRowDates(row, issueId, dates)) };
}

/**
 * Patch one issue's dates inside any cached shape: a bare row list, a
 * detail object, a flat list envelope, or a grouped envelope whose
 * buckets each carry rows. Unknown shapes pass through untouched.
 */
export function withIssueDates(data: unknown, issueId: string, dates: IssueDatesInput): unknown {
  if (Array.isArray(data)) return data.map((row) => withRowDates(row, issueId, dates));
  if (!isRecord(data)) return data;
  if (typeof data.id === "string") return withRowDates(data, issueId, dates);
  if (Array.isArray(data.results)) {
    return { ...data, results: data.results.map((row) => withRowDates(row, issueId, dates)) };
  }
  if (isRecord(data.results)) {
    const buckets = Object.fromEntries(
      Object.entries(data.results).map(([key, bucket]) => [key, withBucketDates(bucket, issueId, dates)])
    );
    return { ...data, results: buckets };
  }
  return data;
}

/** Persist one issue's start and target dates with optimistic apply and rollback. */
export function useUpdateIssueDates(workspaceSlug: string, projectId: string, issueId: string) {
  const queryClient = useQueryClient();
  const mutation = useMutation<void, Error, IssueDatesInput>({
    mutationFn: async (input) => {
      const snapshots = snapshotQueries(queryClient, issueKeys.all(workspaceSlug));
      for (const snapshot of snapshots) {
        queryClient.setQueryData(snapshot.queryKey, withIssueDates(snapshot.data, issueId, input));
      }
      try {
        await updateIssue(getAppClient(), workspaceSlug, projectId, issueId, {
          start_date: input.startDate,
          target_date: input.endDate,
        });
      } catch (error) {
        rollbackSnapshots(queryClient, snapshots);
        notifyError(ISSUE_DATES_FAILED_TITLE, ISSUE_DATES_FAILED_BODY);
        throw error;
      }
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: issueKeys.all(workspaceSlug) });
    },
  });
  return {
    isSaving: mutation.isPending,
    updateIssueDates: (input: IssueDatesInput) => mutation.mutate(input),
  };
}
