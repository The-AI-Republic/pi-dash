// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timeline dependency reads and writes (parity/specs/issues.md §20/§23,
// ISS-237). The layer is cloud-only: with the flag off the hook runs no
// read at all. With it on, every drawn row's finish-to-start relations are
// read, merged into one edge list and deduplicated, and a committed link
// writes the edge from the waiting item's side before the reads refresh.

import { useMutation, useQueries, useQueryClient } from "@tanstack/react-query";
import { addIssueRelations, listIssueRelations } from "@pidash/api-client";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { REFERENCE_STALE_TIME_MS, workspaceScope } from "../../../core/query/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { isEditionFlagOn, GANTT_DEPENDENCIES_FLAG } from "../edition/flags.js";
import { edgesOf, mergeEdges, type DependencyEdge } from "./dependencies.js";

/** Read key for one item's relations in the timeline. */
export function ganttDependencyKey(workspaceSlug: string, projectId: string, issueId: string) {
  return [...workspaceScope(workspaceSlug), "issues", "gantt-dependencies", projectId, issueId] as const;
}

/** Prefix covering every dependency read of one project. */
export function ganttDependencyPrefix(workspaceSlug: string, projectId: string) {
  return [...workspaceScope(workspaceSlug), "issues", "gantt-dependencies", projectId] as const;
}

/** Rows whose relations are worth reading: drawn, confirmed items only. */
export function dependencySourceIds(rows: readonly { id: string; unconfirmed: boolean }[]): string[] {
  return rows.filter((row) => !row.unconfirmed).map((row) => row.id);
}

export type DependencyWrite = (input: { fromId: string; toId: string }) => void;

/** Edges drawn between the given rows plus the write behind a drag-link. */
export function useGanttDependencies(input: {
  workspaceSlug: string;
  projectId: string;
  rows: readonly { id: string; unconfirmed: boolean }[];
}): { edges: DependencyEdge[]; enabled: boolean; link: DependencyWrite; linking: boolean } {
  const enabled = isEditionFlagOn(GANTT_DEPENDENCIES_FLAG);
  const queryClient = useQueryClient();
  const ids = React.useMemo(() => (enabled ? dependencySourceIds(input.rows) : []), [enabled, input.rows]);
  const reads = useQueries({
    queries: ids.map((issueId) => ({
      queryKey: ganttDependencyKey(input.workspaceSlug, input.projectId, issueId),
      queryFn: () => listIssueRelations(getAppClient(), input.workspaceSlug, input.projectId, issueId),
      staleTime: REFERENCE_STALE_TIME_MS,
      enabled,
    })),
  });
  const drawn = React.useMemo(() => new Set(ids), [ids]);
  const edges = React.useMemo(() => {
    const perItem = reads.map((read, index) => {
      const id = ids[index];
      if (id === undefined || read.data === undefined) return [];
      return edgesOf(id, read.data).filter((edge) => drawn.has(edge.fromId));
    });
    return mergeEdges(perItem);
  }, [drawn, ids, reads]);

  const linkMutation = useMutation({
    mutationFn: ({ fromId, toId }: { fromId: string; toId: string }) =>
      addIssueRelations(getAppClient(), input.workspaceSlug, input.projectId, toId, {
        relationType: "finish_after",
        issues: [fromId],
      }),
    onSuccess: () => notifySuccess("Dependency linked"),
    onError: (error: Error) => notifyError("Couldn't link the dependency", error.message),
    onSettled: () =>
      queryClient.invalidateQueries({
        queryKey: ganttDependencyPrefix(input.workspaceSlug, input.projectId),
      }),
  });

  const write = linkMutation.mutate;
  const link = React.useCallback(
    (next: { fromId: string; toId: string }) => {
      write(next);
    },
    [write]
  );

  return { edges, enabled, linking: linkMutation.isPending, link };
}
