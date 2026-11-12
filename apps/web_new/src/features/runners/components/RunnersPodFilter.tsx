// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pod filter affordance for the runner table (NEWFRONT-540, RUN-012). The
// tiles themselves live with the pods section; this is the clear-filter
// control the overview shows above the table while a pod is selected.

import { Button } from "@pidash/kit";
import * as React from "react";

import type { Pod } from "@pidash/api-client";

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
