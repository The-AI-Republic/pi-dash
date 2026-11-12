// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycles list header (parity/specs/cycles.md §6, CYC-014, CYC-021). The
// creation action on the right; search and filter controls dock below
// once their slices land. The button renders for members and admins
// only — guests and viewers get no creation affordance here.

import { Button } from "@pidash/kit";
import { Plus } from "lucide-react";
import * as React from "react";

export interface CyclesListHeaderProps {
  /** Member or admin standing: the button is absent without it. */
  canCreate: boolean;
  /** Opens the create dialog. Slot owner: NEWFRONT-710. */
  onCreateCycle?: (() => void) | undefined;
  /** Expanding name search. Slot owner: NEWFRONT-708. */
  searchControl?: React.ReactNode;
  /** Filters trigger. Slot owner: NEWFRONT-709. */
  filterControl?: React.ReactNode;
}

export function CyclesListHeader({
  canCreate,
  onCreateCycle,
  searchControl,
  filterControl,
}: CyclesListHeaderProps): React.ReactElement {
  return (
    <div data-testid="cycles-list-header" className="flex flex-col gap-(--space-3)">
      <div className="flex items-center justify-end gap-(--space-3)">
        {canCreate ? (
          <Button variant="primary" size="small" onClick={onCreateCycle}>
            <Plus size={16} strokeWidth={1.5} aria-hidden="true" />
            Add cycle
          </Button>
        ) : null}
      </div>
      {searchControl === undefined && filterControl === undefined ? null : (
        <div data-testid="cycles-list-controls" className="flex items-center gap-(--space-2)">
          {searchControl}
          {filterControl}
        </div>
      )}
    </div>
  );
}
