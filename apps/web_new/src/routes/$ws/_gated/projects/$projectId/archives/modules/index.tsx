// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archived-modules tab route (parity/specs/modules.md §7.3, MOD-027;
// parity/specs/archives.md §1). Thin: the per-tab browser title, the shared
// shell, and the modules area's archived screen in the shell slot. The
// ?peekModule= key opens one archived module's quick look over the list
// without leaving /archives/modules; closing removes only that key, and
// every other address parameter survives either way. A direct address with
// the modules flag off lands on the work-items tab instead of an empty
// frame, the same outcome the strip offers.

import { createFileRoute, Navigate, useNavigate } from "@tanstack/react-router";
import * as React from "react";

import type { ArchivedModulesScreen as ArchivedModulesScreenComponent } from "../../../../../../../features/archives/components/ArchivedModulesScreen.js";
import { ArchivesShell } from "../../../../../../../features/archives/components/ArchivesShell.js";
import { archivesRouteTitle, archivesTabHref } from "../../../../../../../features/archives/tabs.js";
import { useProjectRow } from "../../../../../../../features/auth/queries.js";
import { parseModulesListSearch } from "../../../../../../../features/modules/lib/moduleListSearch.js";
import { withModulePeek } from "../../../../../../../features/modules/lib/modulePeek.js";
import { useDocumentHead } from "../../../../../../../shared/shell/document.js";

// Deferred (size gate): the eager route definition only needs the loader;
// the screen rides its own chunk, so the archived rows, the peek and the
// modules header controls stay out of the initial bundle.
const ArchivedModulesScreen = React.lazy(() =>
  import("../../../../../../../features/archives/components/ArchivedModulesScreen.js").then((module) => ({
    default: module.ArchivedModulesScreen,
  }))
) as typeof ArchivedModulesScreenComponent;

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/archives/modules/")({
  validateSearch: (search: Record<string, unknown>) => parseModulesListSearch(search),
  component: ArchivedModulesTab,
});

function ArchivedModulesTab(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  const search = Route.useSearch();
  const navigate = useNavigate();
  const project = useProjectRow(ws, projectId);
  const row = project.data;
  useDocumentHead({
    title: archivesRouteTitle(row?.name, "modules"),
    robots: "noindex, nofollow",
  });
  const peekModuleId = search.peekModule ?? null;
  const openPeek = React.useCallback(
    (moduleId: string | null): void => {
      void navigate({ to: ".", search: withModulePeek(search, moduleId), replace: true } as never);
    },
    [navigate, search]
  );
  if (row !== undefined && !row.module_view) {
    return <Navigate to={archivesTabHref(ws, projectId, "issues")} />;
  }
  return (
    <ArchivesShell workspaceSlug={ws} projectId={projectId} activeTab="modules">
      <React.Suspense fallback={null}>
        <ArchivedModulesScreen
          workspaceSlug={ws}
          projectId={projectId}
          peekModuleId={peekModuleId}
          onPeekChange={openPeek}
        />
      </React.Suspense>
    </ArchivesShell>
  );
}
