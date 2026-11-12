// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Gone-away view (parity/specs/cycles.md §5, CYC-013): a missing,
// deleted, foreign, or archived-via-live-address cycle renders this
// dedicated view — never a generic 404, never the layout chrome, never
// an auto-redirect — with a working route back to the cycles list.
// Slice F mounts the detail gate inside the full detail host.

import { EmptyState } from "@pidash/kit";
import * as React from "react";

export interface CycleGoneAwayProps {
  onBack: () => void;
}

export function CycleGoneAway({ onBack }: CycleGoneAwayProps): React.ReactElement {
  return (
    <div data-testid="cycles-gone-away">
      <EmptyState
        variant="detailed"
        art={{ category: "general", type: "empty" }}
        title="Cycle does not exist"
        description="This cycle was deleted, or the address points at another project. Go back to the project's cycles list."
        primaryAction={{ label: "View other cycles", onSelect: () => onBack() }}
      />
    </div>
  );
}
