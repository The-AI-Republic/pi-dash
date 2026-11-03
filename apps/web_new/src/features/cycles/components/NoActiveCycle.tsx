// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// No-active-cycle hero view (parity/specs/cycles.md §15, CYC-042).
// With no cycle covering today the hero panel explains so instead of
// the row and cards; members and admins get the creation shortcut,
// guests get the explanation alone.

import { Button } from "@pidash/kit";
import * as React from "react";

import { CreateCycleDialog, type CycleProjectRef } from "../dialog/CycleDialog.js";

/** Empty title; the oracle matches /active cycle/ on it. */
export const NO_ACTIVE_CYCLE_TITLE = "No active cycle";
/** Empty description; the oracle matches /today/ on it. */
export const NO_ACTIVE_CYCLE_DESCRIPTION = "There is no cycle covering today.";

export interface NoActiveCycleProps {
  workspaceSlug: string;
  projectId: string;
  /** Members and admins create; guests read the explanation alone. */
  canCreate: boolean;
  currentProject: CycleProjectRef;
}

export function NoActiveCycle({
  workspaceSlug,
  projectId,
  canCreate,
  currentProject,
}: NoActiveCycleProps): React.ReactElement {
  const [createOpen, setCreateOpen] = React.useState(false);
  return (
    <div data-testid="no-active-cycle" className="flex flex-col gap-(--space-3) px-(--space-4) py-(--space-2)">
      <h2 className="text-emphasis text-(--text)">{NO_ACTIVE_CYCLE_TITLE}</h2>
      <p className="text-body text-(--text-muted)">{NO_ACTIVE_CYCLE_DESCRIPTION}</p>
      {canCreate ? (
        <>
          <div>
            <Button variant="primary" size="small" onClick={() => setCreateOpen(true)}>
              Create cycle
            </Button>
          </div>
          <CreateCycleDialog
            open={createOpen}
            onOpenChange={setCreateOpen}
            workspaceSlug={workspaceSlug}
            projectId={projectId}
            currentProject={currentProject}
          />
        </>
      ) : null}
    </div>
  );
}
