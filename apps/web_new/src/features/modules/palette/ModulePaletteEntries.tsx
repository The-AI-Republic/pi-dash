// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Modules palette wiring (parity/specs/modules.md §10.2, MOD-037/040).
// Mounted by the project layout, so the two entries exist on every page of
// one project and nowhere else: the create command opens the modules
// area's own create dialog over whatever is on screen, and the jump command
// steps into the palette's module page. Everything the gates need — the
// project row's modules switch and the caller's project standing — is read
// here and re-registered whenever it resolves, so the entries appear and
// disappear with the context instead of being rendered disabled.
//
// The dialog rides the modules feature's own chunk: a page that never uses
// the command carries neither the create form nor its pickers.

import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { registerCommands, registerPalettePages, type PidashCommand } from "../../../shared/commands/index.js";
import { useProjectRoles } from "../../auth/index.js";
import { isProjectAuthorized } from "../../projects/index.js";
import { moduleProjectFactsQueryOptions, moduleProjectIdentifier } from "../api/projectFacts.js";
import { moduleDetailHref } from "../lib/moduleList.js";
import { buildModuleCreateCommand, buildModuleJumpCommand } from "./moduleCommands.js";
import { projectModulesJumpPage } from "./moduleJumpPage.js";

const LazyModuleSaveDialog = React.lazy(() =>
  import("../components/dialogs/ModuleSaveDialog.js").then((module) => ({ default: module.ModuleSaveDialog }))
);

export interface ModulePaletteEntriesProps {
  workspaceSlug: string;
  projectId: string;
}

interface CreateSession {
  /** Bumped per open so a reopened dialog starts from defaults again. */
  key: number;
}

export function ModulePaletteEntries({
  workspaceSlug,
  projectId,
}: ModulePaletteEntriesProps): React.ReactElement | null {
  const navigate = useNavigate();
  const project = useQuery(moduleProjectFactsQueryOptions(getAppClient(), workspaceSlug, projectId));
  const roles = useProjectRoles(workspaceSlug);
  const [session, setSession] = React.useState<CreateSession | null>(null);
  const nextKey = React.useRef(0);

  const modulesEnabled = project.data?.module_view === true;
  const canCreate = isProjectAuthorized(roles.data?.[projectId] ?? null);
  const projectName = project.data?.name ?? projectId;

  const openCreate = React.useCallback(() => {
    nextKey.current += 1;
    setSession({ key: nextKey.current });
  }, []);

  // No dependency array: a late project row or role map re-registers the
  // entries, which is how both gates take effect without a reload.
  React.useEffect(() =>
    registerCommands(
      [
        buildModuleCreateCommand({ projectName, modulesEnabled, canCreate, onCreate: openCreate }),
        buildModuleJumpCommand({ projectName, modulesEnabled }),
      ].filter((command): command is PidashCommand => command !== null)
    )
  );

  React.useEffect(() =>
    registerPalettePages([
      projectModulesJumpPage({
        workspaceSlug,
        projectId,
        onPick: (moduleId: string) => {
          void navigate({ to: moduleDetailHref(workspaceSlug, projectId, moduleId) });
        },
      }),
    ])
  );

  if (session === null) return null;
  return (
    <React.Suspense fallback={null}>
      <LazyModuleSaveDialog
        key={session.key}
        open
        mode="create"
        workspaceSlug={workspaceSlug}
        currentProject={{
          id: projectId,
          name: projectName,
          identifier: moduleProjectIdentifier(project.data),
        }}
        onOpenChange={(next) => {
          if (!next) setSession(null);
        }}
      />
    </React.Suspense>
  );
}
