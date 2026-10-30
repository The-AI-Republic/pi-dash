// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycles feature gate (parity/specs/cycles.md §5, CYC-010). Replaces the
// whole list while the project's cycles switch is off. The explanation
// is visible to every role; the management shortcut stays rendered but
// disabled without admin standing.

import { EmptyState } from "@pidash/kit";
import * as React from "react";

export interface CyclesFeatureGateProps {
  /** Project admin standing: the shortcut stays disabled without it. */
  isAdmin: boolean;
  onManageFeatures: () => void;
}

export function CyclesFeatureGate({ isAdmin, onManageFeatures }: CyclesFeatureGateProps): React.ReactElement {
  return (
    <div data-testid="cycles-feature-gate">
      <EmptyState
        variant="detailed"
        art={{ category: "general", type: "disabled" }}
        title="Cycles is not enabled for this project."
        description="Break work down by timeboxed chunks, work backwards from your project deadline to set dates, and make tangible progress as a team. Enable the cycles feature for your project to start using them."
        primaryAction={{ label: "Manage features", onSelect: () => onManageFeatures(), disabled: !isAdmin }}
      />
    </div>
  );
}
