// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project cycles tab home (parity/specs/cycles.md §1–§5). The cycles
// area owns this screen: slice A (NEWFRONT-707) built the list shell
// with its feature gate, zero-state, skeleton and error; slice B
// (NEWFRONT-708) fills the list body (hero slot, groups, rows, search,
// no-match) and owns the ?peekCycle= address key the quick look opens
// through. The create dialog slot (NEWFRONT-710) lands on the screen.

import { createFileRoute, useNavigate } from "@tanstack/react-router";
import * as React from "react";

// Direct module imports: the feature barrel grows with later slices
// (dialogs, pickers), which must not join this route's initial chunk.
import { CyclesListBody } from "../../../../../../features/cycles/components/CyclesListBody.js";
import { CyclesListScreen } from "../../../../../../features/cycles/components/CyclesListScreen.js";
import { parseCyclesListSearch } from "../../../../../../features/cycles/lib/cycleListSearch.js";

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/cycles/")({
  validateSearch: (search: Record<string, unknown>) => parseCyclesListSearch(search),
  component: CyclesComponent,
});

function CyclesComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  const search = Route.useSearch();
  const navigate = useNavigate();
  const peekCycleId = search.peekCycle ?? null;
  const setPeekCycleId = (cycleId: string | null): void => {
    const next = { ...search };
    if (cycleId === null) delete next.peekCycle;
    else next.peekCycle = cycleId;
    void navigate({ to: ".", search: next, replace: true } as never);
  };
  return (
    <CyclesListScreen workspaceSlug={ws} projectId={projectId}>
      <CyclesListBody
        workspaceSlug={ws}
        projectId={projectId}
        peekCycleId={peekCycleId}
        onPeekChange={setPeekCycleId}
      />
    </CyclesListScreen>
  );
}
