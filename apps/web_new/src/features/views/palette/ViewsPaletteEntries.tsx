// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Views palette wiring (parity/specs/views.md §12, VIEW-042). Mounted by
// the project layout route, so the entries live on every project page:
// creation opens the views-owned dialog over whatever is open, and the
// jump menu navigates to the picked view's detail. Registers the
// area-owned commands and the jump sub-page through the shell registry;
// the shell owns the sequence firing, the palette frame and the search
// box. The dialog rides a lazy chunk so project pages never carry it
// until the command fires.

import { useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { registerCommands, registerPalettePages, type PidashCommand } from "../../../shared/commands/index.js";
import { useProjectRoles, useProjectRow } from "../../auth/index.js";
import { isProjectAuthorized } from "../../projects/index.js";
import { projectViewHref } from "../lib/projectViews.js";
import { projectViewsJumpPage } from "./viewJumpPage.js";
import { buildViewCreateCommand, buildViewJumpCommand } from "./viewCommands.js";

const LazyCreateViewDialog = React.lazy(() =>
  import("../dialog/CreateViewDialog.js").then((module) => ({ default: module.CreateViewDialog }))
);

export interface ViewsPaletteEntriesProps {
  workspaceSlug: string;
  projectId: string;
}

/**
 * Registers the views palette entries for one project. Renders nothing
 * until the create command fires. Effects run every render (no
 * dependency array) so late-loading inputs — the project row, the
 * roles — re-register as they resolve, and the entries appear and
 * disappear as the context and flags load.
 */
export function ViewsPaletteEntries({ workspaceSlug, projectId }: ViewsPaletteEntriesProps): React.ReactElement | null {
  const navigate = useNavigate();
  const project = useProjectRow(workspaceSlug, projectId);
  const roles = useProjectRoles(workspaceSlug);
  const [createOpen, setCreateOpen] = React.useState(false);

  const viewsEnabled = project.data?.issue_views_view ?? false;
  const canCreateView = isProjectAuthorized(roles.data?.[projectId] ?? null);
  const projectName = project.data?.name ?? projectId;
  // The dialog's cycle/module entries hide while their project features
  // are off (VIEW-003); unknown while loading reads as shown.
  const hideCycle = project.data?.cycle_view === false;
  const hideModule = project.data?.module_view === false;

  React.useEffect(() =>
    registerCommands(
      [
        buildViewCreateCommand({
          projectName,
          viewsEnabled,
          canCreateView,
          onCreateView: () => setCreateOpen(true),
        }),
        buildViewJumpCommand({ projectName, viewsEnabled }),
      ].filter((command): command is PidashCommand => command !== null)
    )
  );

  React.useEffect(() =>
    registerPalettePages([
      projectViewsJumpPage({
        workspaceSlug,
        projectId,
        onPick: (viewId: string) => {
          void navigate({ to: projectViewHref(workspaceSlug, projectId, viewId) });
        },
      }),
    ])
  );

  if (!createOpen) return null;
  return (
    <React.Suspense fallback={null}>
      <LazyCreateViewDialog
        open={createOpen}
        onOpenChange={setCreateOpen}
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        hideCycle={hideCycle}
        hideModule={hideModule}
      />
    </React.Suspense>
  );
}
