/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useMemo, useState } from "react";
import type { IPod, IRunner } from "@pi-dash/types";

/** State + filtering for "click pod tile to filter runners".
 *
 * Self-heals when the selected pod disappears from ``pods`` (deleted
 * server-side, workspace switch). Without that the filter would silently
 * keep an unreachable id and hide every runner.
 */
export function useSelectedPodFilter(runners: IRunner[] | undefined, pods: IPod[] | undefined) {
  const [selectedPodId, setSelectedPodId] = useState<string | null>(null);

  useEffect(() => {
    if (!selectedPodId) return;
    if (!pods) return;
    if (!pods.some((p) => p.id === selectedPodId)) setSelectedPodId(null);
  }, [pods, selectedPodId]);

  const filteredRunners = useMemo(() => {
    if (!runners) return runners;
    if (!selectedPodId) return runners;
    return runners.filter((r) => r.pod === selectedPodId);
  }, [runners, selectedPodId]);

  const selectedPod = selectedPodId ? pods?.find((p) => p.id === selectedPodId) : undefined;

  return { selectedPodId, setSelectedPodId, filteredRunners, selectedPod } as const;
}
