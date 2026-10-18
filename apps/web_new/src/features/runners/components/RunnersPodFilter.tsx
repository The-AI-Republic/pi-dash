// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pod filter affordance for the runner table (NEWFRONT-540, RUN-012). The
// pods slice owns the tiles; this is the clear-filter control the overview
// shows above the table, and the tile button the pods slice reuses.

import { Button } from "@pidash/kit";
import * as React from "react";

import type { Pod } from "@pidash/api-client";

export interface PodFilterTilesProps {
  pods: readonly Pod[];
  selectedPodId: string | null;
  onToggle: (podId: string) => void;
}

/** One tile per pod: name, default marker and runner count. */
export function PodFilterTiles({ pods, selectedPodId, onToggle }: PodFilterTilesProps): React.ReactElement {
  return (
    <div className="flex flex-wrap gap-(--space-2)">
      {pods.map((pod) => (
        <button
          key={pod.id}
          type="button"
          aria-pressed={pod.id === selectedPodId}
          aria-label={`Filter runners by pod ${pod.name}`}
          onClick={() => onToggle(pod.id)}
          className="text-meta rounded-(--radius-control) border border-(--border) px-(--space-3) py-(--space-2) text-left hover:border-(--text-muted)"
        >
          <span className="block font-medium text-(--text)">{pod.name}</span>
          <span className="block text-(--text-muted)">
            {pod.is_default ? "default · " : ""}
            {pod.runner_count} runner(s)
          </span>
        </button>
      ))}
    </div>
  );
}

export interface PodFilterBarProps {
  selectedPod: Pod;
  onClear: () => void;
}

/** "Filtering by <pod>" plus its clear control. */
export function PodFilterBar({ selectedPod, onClear }: PodFilterBarProps): React.ReactElement {
  return (
    <div className="text-meta flex items-center gap-(--space-2) text-(--text-muted)">
      <span>Filtering runners by pod {selectedPod.name}</span>
      <Button size="small" variant="ghost" onClick={onClear}>
        Clear filter
      </Button>
    </div>
  );
}
