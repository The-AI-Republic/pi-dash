// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timeline writes (parity/specs/issues.md §6, ISS-050–055). The chart's
// gestures persist through two seams: moves and resizes send one batch
// issue-dates request, while reorders and planted blocks use the single
// issue PATCH. Every write paints its list caches optimistically and rolls
// them back with an error notice when the server refuses.

import { useMutation, useQueryClient } from "@tanstack/react-query";

import { bulkUpdateIssueDates, updateIssue } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { rollbackSnapshots, snapshotQueries } from "../../../core/query/optimistic.js";
import { notifyError } from "../../../shared/shell/index.js";
import type { IssueListItem } from "@pidash/api-client";
import { withIssueDates, type IssueDatesInput } from "../api/dates.js";
import { issueKeys } from "../api/keys.js";

/** Notice when a gesture's date write is refused. */
export const GANTT_DATES_FAILED_TITLE = "Couldn't save the dates";
const GANTT_DATES_FAILED_BODY = "The timeline is unchanged — check your connection and try again.";
/** Notice when a sidebar reorder is refused. */
export const GANTT_REORDER_FAILED_TITLE = "Couldn't save the order";
const GANTT_REORDER_FAILED_BODY = "The order is unchanged — check your connection and try again.";

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

/** Reorder every cached list's rows to the given id sequence. */
export function withIssueOrder(data: unknown, order: readonly string[]): unknown {
  if (!isRecord(data) || !Array.isArray(data.results)) return data;
  const rank = new Map(order.map((id, index) => [id, index]));
  const results = [...(data.results as IssueListItem[])].sort((a, b) => {
    const left = rank.get(a.id);
    const right = rank.get(b.id);
    if (left === undefined || right === undefined) return 0;
    return left - right;
  });
  return { ...data, results };
}

export interface GanttMutations {
  /** Move a bar and persist both ends in one batch (ISS-051). */
  moveBar: (issueId: string, dates: IssueDatesInput) => void;
  /** Resize one end and persist both ends in one batch (ISS-052/053). */
  resizeBar: (issueId: string, dates: IssueDatesInput) => void;
  /** Plant a block on an undated row through the single PATCH (ISS-054). */
  plantBlock: (issueId: string, dates: IssueDatesInput) => void;
  /** Persist a manual sort rank (ISS-050). */
  reorder: (issueId: string, sortOrder: number, order: readonly string[]) => void;
  /** True while any timeline write is in flight. */
  isSaving: boolean;
}

/**
 * Writes behind the timeline. Dates share one mutation because move,
 * resize and plant differ only in which endpoint receives the same pair;
 * reorder is its own PATCH.
 */
export function useGanttMutations(workspaceSlug: string, projectId: string): GanttMutations {
  const queryClient = useQueryClient();
  const listsPrefix = issueKeys.lists(workspaceSlug, projectId);

  const dates = useMutation<void, Error, { issueId: string; input: IssueDatesInput; batch: boolean }>({
    mutationFn: async ({ issueId, input, batch }) => {
      // A list read that started before the drop can resolve after the
      // optimistic paint and restore the pre-drag dates. Stop those reads
      // first so the bar settles on the dates this write commits (ISS-051).
      await queryClient.cancelQueries({ queryKey: listsPrefix });
      const snapshots = snapshotQueries(queryClient, listsPrefix);
      for (const snapshot of snapshots) {
        queryClient.setQueryData(snapshot.queryKey, withIssueDates(snapshot.data, issueId, input));
      }
      try {
        if (batch) {
          await bulkUpdateIssueDates(getAppClient(), workspaceSlug, projectId, [
            {
              id: issueId,
              ...(input.startDate === null ? {} : { start_date: input.startDate }),
              ...(input.endDate === null ? {} : { target_date: input.endDate }),
            },
          ]);
        } else {
          await updateIssue(getAppClient(), workspaceSlug, projectId, issueId, {
            start_date: input.startDate,
            target_date: input.endDate,
          });
        }
        // The batch endpoint commits this exact pair. Reassert it after the
        // write in case a read raced the request's success (ISS-051).
        for (const snapshot of snapshots) {
          queryClient.setQueryData(
            snapshot.queryKey,
            withIssueDates(queryClient.getQueryData(snapshot.queryKey), issueId, input)
          );
        }
      } catch (error) {
        rollbackSnapshots(queryClient, snapshots);
        notifyError(GANTT_DATES_FAILED_TITLE, GANTT_DATES_FAILED_BODY);
        throw error;
      }
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: listsPrefix }),
  });

  const order = useMutation<void, Error, { issueId: string; sortOrder: number; order: readonly string[] }>({
    mutationFn: async ({ issueId, sortOrder, order: nextOrder }) => {
      const snapshots = snapshotQueries(queryClient, listsPrefix);
      for (const snapshot of snapshots) {
        queryClient.setQueryData(snapshot.queryKey, withIssueOrder(snapshot.data, nextOrder));
      }
      try {
        await updateIssue(getAppClient(), workspaceSlug, projectId, issueId, { sort_order: sortOrder });
      } catch (error) {
        rollbackSnapshots(queryClient, snapshots);
        notifyError(GANTT_REORDER_FAILED_TITLE, GANTT_REORDER_FAILED_BODY);
        throw error;
      }
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: listsPrefix }),
  });

  return {
    moveBar: (issueId, input) => dates.mutate({ issueId, input, batch: true }),
    resizeBar: (issueId, input) => dates.mutate({ issueId, input, batch: true }),
    plantBlock: (issueId, input) => dates.mutate({ issueId, input, batch: false }),
    reorder: (issueId, sortOrder, nextOrder) => order.mutate({ issueId, sortOrder, order: nextOrder }),
    isSaving: dates.isPending || order.isPending,
  };
}
