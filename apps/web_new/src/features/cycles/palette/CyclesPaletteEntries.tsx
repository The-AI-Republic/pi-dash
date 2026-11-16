// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycles palette wiring (parity/specs/cycles.md §6, CYC-014). Mounted by
// the project layout, so the entry exists on every page of one project
// and nowhere else: it opens the cycles area's own create dialog over
// whatever is on screen. Everything the gates need — the project row's
// cycles switch and the caller's project standing — is read here and
// re-registered whenever it resolves, so the entry appears and
// disappears with the context instead of being rendered disabled.
// This is distinct from CyclePaletteEntries (components/), which
// registers the per-cycle contextual entries (favorite, copy-URL).

import * as React from "react";

import { registerCommands, type PidashCommand } from "../../../shared/commands/index.js";
import { useProjectRoles, useProjectRow } from "../../auth/index.js";
import { isProjectAuthorized } from "../../projects/index.js";
import { buildCycleCreateCommand } from "./cycleCommands.js";

const LazyCreateCycleDialog = React.lazy(() =>
  import("../dialog/CycleDialog.js").then((module) => ({ default: module.CreateCycleDialog }))
);

export interface CyclesPaletteEntriesProps {
  workspaceSlug: string;
  projectId: string;
}

export function CyclesPaletteEntries({
  workspaceSlug,
  projectId,
}: CyclesPaletteEntriesProps): React.ReactElement | null {
  const project = useProjectRow(workspaceSlug, projectId);
  const roles = useProjectRoles(workspaceSlug);
  const [createOpen, setCreateOpen] = React.useState(false);

  const cyclesEnabled = project.data?.cycle_view === true;
  const canCreate = isProjectAuthorized(roles.data?.[projectId] ?? null);
  const projectName = project.data?.name ?? projectId;
  const projectIdentifier = project.data?.identifier ?? "";

  // No dependency array: a late project row or role map re-registers the
  // entry, which is how both gates take effect without a reload.
  React.useEffect(() =>
    registerCommands(
      [
        buildCycleCreateCommand({
          projectName,
          cyclesEnabled,
          canCreate,
          onCreate: () => setCreateOpen(true),
        }),
      ].filter((command): command is PidashCommand => command !== null)
    )
  );

  if (!createOpen) return null;
  return (
    <React.Suspense fallback={null}>
      <LazyCreateCycleDialog
        open={createOpen}
        onOpenChange={setCreateOpen}
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        currentProject={{ id: projectId, name: projectName, identifier: projectIdentifier }}
      />
    </React.Suspense>
  );
}
