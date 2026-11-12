// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner pod filter (NEWFRONT-540, RUN-012). The overview narrows its table
// to one pod without a second fetch: the selected pod id filters the rows
// client-side. The selection heals itself — it clears when the pod leaves
// the loaded pod list (deleted, or another scope) so the table is never
// stranded on a pod that no longer applies.

import * as React from "react";

import type { Pod, Runner } from "@pidash/api-client";

/** Rows whose pod matches, or every row without a selection. */
export function filterRunnersByPod(runners: readonly Runner[], podId: string | null): Runner[] {
  return podId === null ? [...runners] : runners.filter((runner) => runner.pod === podId);
}

/** The selected pod row, when the list still carries it. */
export function selectPod(pods: readonly Pod[] | undefined, podId: string | null): Pod | null {
  if (podId === null || pods === undefined) return null;
  return pods.find((pod) => pod.id === podId) ?? null;
}

export interface PodFilterState {
  selectedPodId: string | null;
  selectedPod: Pod | null;
  runners: Runner[];
  /** Toggle one tile: selecting the active pod clears the filter. */
  toggle: (podId: string) => void;
  clear: () => void;
}

/**
 * Wire the selection to the loaded rows. `podsLoaded` marks a settled pod
 * list (success); the selection clears only then, never mid-load, so a
 * refetch cannot wipe a valid filter.
 */
export function useRunnerPodFilter(args: {
  runners: readonly Runner[] | undefined;
  pods: readonly Pod[] | undefined;
  podsLoaded: boolean;
}): PodFilterState {
  const [selectedPodId, setSelectedPodId] = React.useState<string | null>(null);
  const exists = selectedPodId === null || (args.pods ?? []).some((pod) => pod.id === selectedPodId);
  React.useEffect(() => {
    if (args.podsLoaded && !exists) setSelectedPodId(null);
  }, [args.podsLoaded, exists]);
  const effective = exists ? selectedPodId : null;
  return {
    selectedPodId: effective,
    selectedPod: selectPod(args.pods, effective),
    runners: filterRunnersByPod(args.runners ?? [], effective),
    toggle: (podId) => setSelectedPodId((current) => (current === podId ? null : podId)),
    clear: () => setSelectedPodId(null),
  };
}
