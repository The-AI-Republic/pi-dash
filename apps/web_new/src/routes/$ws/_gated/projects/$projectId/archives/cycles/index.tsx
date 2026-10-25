// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archived-cycles tab route (parity/specs/archives.md §1 and §4,
// ARCH-001–003, ARCH-014–019). Thin: the per-tab browser title plus
// the shared shell with the archived-cycles screen mounted in its
// slot. The ?peekCycle= address key opens one archived cycle's quick
// look over the list; closing clears it back to the plain tab
// address. A direct address with the cycles flag off lands on the
// work-items tab instead of an empty frame, the same outcome the
// strip offers.

import { createFileRoute, Navigate, useNavigate } from "@tanstack/react-router";
import * as React from "react";

// The screen is deferred: this route definition is eager, and its cycle
// barrel would pin the archived list, rows and transfer dialog into the
// initial chunk (size gate). The route awaits the chunk, so a visit still
// commits the screen in one pass.
const loadArchivedCyclesScreen = () =>
  import("../../../../../../../features/archives/components/ArchivedCyclesScreen.js").then((module) => ({
    default: module.ArchivedCyclesScreen,
  }));
const ArchivedCyclesScreen = React.lazy(loadArchivedCyclesScreen);

import { ArchivesShell } from "../../../../../../../features/archives/components/ArchivesShell.js";
import { archivesRouteTitle, archivesTabHref } from "../../../../../../../features/archives/tabs.js";
import { useProjectRow } from "../../../../../../../features/auth/queries.js";
import { parseCyclesListSearch } from "../../../../../../../features/cycles/lib/cycleListSearch.js";
import { useDocumentHead } from "../../../../../../../shared/shell/document.js";

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/archives/cycles/")({
  validateSearch: (search: Record<string, unknown>) => parseCyclesListSearch(search),
  beforeLoad: async () => {
    await loadArchivedCyclesScreen();
  },
  component: ArchivedCyclesTab,
});

function ArchivedCyclesTab(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  const search = Route.useSearch();
  const navigate = useNavigate();
  const project = useProjectRow(ws, projectId);
  const row = project.data;
  useDocumentHead({
    title: archivesRouteTitle(row?.name, "cycles"),
    robots: "noindex, nofollow",
  });
  const peekCycleId = search.peekCycle ?? null;
  const setPeekCycleId = (cycleId: string | null): void => {
    const next = { ...search };
    if (cycleId === null) delete next.peekCycle;
    else next.peekCycle = cycleId;
    void navigate({ to: ".", search: next, replace: true } as never);
  };
  if (row !== undefined && !row.cycle_view) {
    return <Navigate to={archivesTabHref(ws, projectId, "issues")} />;
  }
  return (
    <ArchivesShell workspaceSlug={ws} projectId={projectId} activeTab="cycles">
      <React.Suspense fallback={null}>
        <ArchivedCyclesScreen
          workspaceSlug={ws}
          projectId={projectId}
          peekCycleId={peekCycleId}
          onPeekChange={setPeekCycleId}
        />
      </React.Suspense>
    </ArchivesShell>
  );
}
