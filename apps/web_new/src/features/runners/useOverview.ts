// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Overview data wiring (NEWFRONT-540, RUN-002/RUN-010). One hook for both
// scopes: it reads the area context, loads the runner list only once access
// allows it, and hands the screen data plus the two row-action writers.

import type { Pod, Runner, RunnerPatch } from "@pidash/api-client";
import { useMe } from "../../core/session/index.js";
import { useRunnersArea } from "./area.js";
import { useDeleteRunner, useRevokeRunner, useUpdateRunner } from "./api/mutations.js";
import { useRunners, useRunnersPods } from "./api/queries.js";
import { useRunnerPodFilter, type PodFilterState } from "./podFilter.js";

export interface RunnersOverviewData extends PodFilterState {
  onDelete: (runnerId: string, options?: { purgeLocal?: boolean }) => Promise<void>;
  onRevoke: (runnerId: string) => Promise<void>;
  /** Pods of the current scope with the read's progress (RUN-012). */
  pods: Pod[] | undefined;
  podsLoading: boolean;
  podsFailed: boolean;
  /** Every runner of the scope, before the pod filter (pod members panel). */
  allRunners: Runner[];
  /** The signed-in user's id; pod actions are owner-only. */
  userId: string | null;
  onUpdateRunner: (runnerId: string, patch: RunnerPatch) => Promise<unknown>;
}

export function useRunnersOverview(workspaceSlug: string): RunnersOverviewData {
  const area = useRunnersArea();
  const scope = area.scope.projectId ?? null;
  const list = useRunners(workspaceSlug, area.workspaceId, scope, area.allowed);
  const pods = useRunnersPods(workspaceSlug, area.workspaceId, scope, area.allowed);
  const remove = useDeleteRunner(workspaceSlug);
  const revoke = useRevokeRunner(workspaceSlug);
  const update = useUpdateRunner(workspaceSlug);
  const me = useMe();
  const filter = useRunnerPodFilter({
    runners: area.allowed ? list.data : undefined,
    pods: pods.data,
    podsLoaded: pods.isSuccess,
  });
  return {
    ...filter,
    // The mutations reject on failure; the screen catches and toasts.
    onDelete: (runnerId, options) =>
      remove
        .mutateAsync(options?.purgeLocal === undefined ? runnerId : { runnerId, purgeLocal: options.purgeLocal })
        .then(() => undefined),
    onRevoke: (runnerId) => revoke.mutateAsync(runnerId).then(() => undefined),
    pods: pods.data,
    podsLoading: area.allowed && pods.isPending,
    podsFailed: area.allowed && pods.isError,
    allRunners: area.allowed ? (list.data ?? []) : [],
    userId: me.data?.id ?? null,
    onUpdateRunner: (runnerId, patch) => update.mutateAsync({ runnerId, patch }),
  };
}
