// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace palette wiring (shell spec §8, SHELL-086 through SHELL-089).
// Registers the shell-owned entry catalogs and picker sub-pages for the
// current workspace context. The route loads this through React.lazy so
// the catalogs ride a deferred chunk instead of the initial shell chain
// (Quality gates budget); builders hide what the caller may not use, so
// registration is the permission gating (SHELL-092). Other areas
// register their own entries beside these from their own chunks. The
// permission and instance reads live here too, keeping them out of the
// initial chain alongside the builders that consume them.

import * as React from "react";

import { useInstanceConfig, usePermissions } from "../../../core/session/index.js";
import { registerCommands, registerPalettePages } from "../../commands/index.js";
import { buildAccountCommands } from "./account.js";
import { buildCreationCommands } from "./creation.js";
import { buildNavigationCommands, type NavigationProject } from "./navigation.js";
import { projectPickerPage, workspacePickerPage, type PickerProject, type PickerWorkspace } from "./navigationPages.js";
import {
  buildPreferenceCommands,
  firstDayPickerPage,
  languagePickerPage,
  themePickerPage,
  timezonePickerPage,
} from "./preferenceEntries.js";

export interface WorkspacePaletteEntriesProps {
  workspaceSlug: string;
  workspaceName: string;
  /** Null outside a project; project-level entries stay hidden then. */
  project: NavigationProject | null;
  projects: PickerProject[];
  workspaces: PickerWorkspace[];
  /** Opens the shell-owned project creation dialog. */
  onOpenCreateProject: () => void;
  /** Routes to the workspace creation page. */
  onCreateWorkspace: () => void;
  /** Client-side navigation to an app href. */
  onNavigate: (href: string) => void;
  /** Routes to the workspace invitations inbox. */
  onWorkspaceInvites: () => void;
  /** Opens a URL outside the app (system browser on desktop). */
  onOpenExternal: (url: string) => void;
  onSelectWorkspace: (slug: string) => void;
  onSelectProject: (projectId: string) => void;
}

/**
 * Registers the workspace entry catalogs. Renders nothing. Effects run
 * every render (no dependency array) so late-loading inputs —
 * permissions, instance flags — re-register as they resolve.
 */
export function WorkspacePaletteEntries(props: WorkspacePaletteEntriesProps): null {
  const {
    workspaceSlug,
    workspaceName,
    project,
    projects,
    workspaces,
    onOpenCreateProject,
    onCreateWorkspace,
    onNavigate,
    onWorkspaceInvites,
    onOpenExternal,
    onSelectWorkspace,
    onSelectProject,
  } = props;
  const permissions = usePermissions(workspaceSlug);
  const instanceConfig = useInstanceConfig();
  const canEdit = permissions.canEdit;
  const canCreateWorkspace = instanceConfig.data?.is_workspace_creation_disabled !== true;

  React.useEffect(() =>
    registerCommands([
      ...buildCreationCommands({
        canCreateProject: canEdit,
        canCreateWorkspace,
        onOpenCreateProject,
        onCreateWorkspace,
      }),
      ...buildNavigationCommands({
        workspaceSlug,
        workspaceName,
        project,
        canEdit,
        navigate: onNavigate,
      }),
      ...buildPreferenceCommands(),
      ...buildAccountCommands({ onWorkspaceInvites, onOpenExternal }),
    ])
  );

  React.useEffect(() =>
    registerPalettePages([
      projectPickerPage({ projects, onPick: onSelectProject }),
      workspacePickerPage({ workspaces, onPick: onSelectWorkspace }),
      themePickerPage(),
      timezonePickerPage(),
      firstDayPickerPage(),
      languagePickerPage(),
    ])
  );

  return null;
}
