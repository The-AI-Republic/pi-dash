// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// First-run zero-state (parity/specs/cycles.md §5, CYC-011). The view is
// visible to every role; the creation shortcut is enabled for members
// and admins only and stays disabled for guests and viewers, without
// error.

import { EmptyState } from "@pidash/kit";
import * as React from "react";

export interface CyclesZeroStateProps {
  /** Member or admin standing: the shortcut stays disabled without it. */
  canCreate: boolean;
  /** Opens the create dialog. Slot owner: NEWFRONT-710. */
  onCreateCycle?: (() => void) | undefined;
}

export function CyclesZeroState({ canCreate, onCreateCycle }: CyclesZeroStateProps): React.ReactElement {
  return (
    <div data-testid="cycles-zero-state">
      <EmptyState
        variant="detailed"
        art={{ category: "general", type: "empty" }}
        title="Plan work in cycles"
        description="A cycle is a dated window that groups work items. Set your first cycle to start planning the project's work."
        primaryAction={{
          label: "Set your first cycle",
          onSelect: () => onCreateCycle?.(),
          disabled: !canCreate,
        }}
      />
    </div>
  );
}
