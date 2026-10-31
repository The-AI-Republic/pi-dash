// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project tab strip container (parity/specs/shell.md §7, SHELL-072–079).
// Reads the workspace mode, the project, its tabs, standing and stored
// preferences, then renders the chrome. The strip mounts in tabbed mode
// only, stays hidden without project data, and fails closed when the
// standing read errors.

import { useQuery } from "@tanstack/react-query";
import { useLocation, useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { useProjectRoles } from "../../auth/index.js";
import {
  isProjectAdmin,
  isProjectAuthorized,
  isTabbedMode,
  parseNavigationPrefs,
  resolveActiveTabKey,
  selectProjectTabs,
  type StripWorkItem,
} from "../tabs.js";
import { projectsQueryOptions } from "./../api/queries.js";
import { useProjectProperties, useWorkspaceProperties } from "./../api/tabPreferences.js";
import { useTabPreferences } from "./../api/tabPreferences.js";
import { ProjectTabChrome } from "./ProjectTabChrome.js";

export interface ProjectTabStripProps {
  workspaceSlug: string;
  projectId: string;
  workItem?: StripWorkItem | null;
}

export function ProjectTabStrip({
  workspaceSlug,
  projectId,
  workItem = null,
}: ProjectTabStripProps): React.ReactElement | null {
  const navigate = useNavigate();
  const { pathname } = useLocation();
  const mode = useWorkspaceProperties(workspaceSlug);
  const projects = useQuery(projectsQueryOptions(getAppClient(), workspaceSlug));
  const stored = useProjectProperties(workspaceSlug, projectId);
  const roles = useProjectRoles(workspaceSlug);
  const prefs = useTabPreferences(workspaceSlug, projectId);

  if (mode.isPending || projects.isPending || stored.isPending || roles.isPending) return null;
  if (mode.isError || projects.isError || roles.isError) return null;
  if (!isTabbedMode(mode.data.navigation_control_preference)) return null;
  const project = projects.data.find((entry) => entry.id === projectId) ?? null;
  if (project === null) return null;
  const role = roles.data[projectId] ?? null;
  const tabs = selectProjectTabs({ workspaceSlug, projectId, flags: project, role });
  const activeKey = resolveActiveTabKey({ tabs, pathname, projectId, workItem });
  const activeHref = tabs.find((tab) => tab.key === activeKey)?.href ?? null;
  const preferences = parseNavigationPrefs(stored.data?.preferences);

  return (
    <ProjectTabChrome
      workspaceSlug={workspaceSlug}
      project={{ id: project.id, name: project.name }}
      joinedProjects={projects.data
        .filter((entry) => entry.member_role !== null)
        .map((entry) => ({ id: entry.id, name: entry.name }))}
      tabs={tabs}
      activeKey={activeKey}
      activeHref={activeHref}
      defaultTab={preferences.defaultTab}
      hiddenTabs={preferences.hiddenTabs}
      isAdmin={isProjectAdmin(role)}
      isAuthorized={isProjectAuthorized(role)}
      onNavigate={(href) => {
        void navigate({ to: href });
      }}
      onToggleDefault={prefs.toggleDefault}
      onHideTab={prefs.hideTab}
      onShowTab={prefs.showTab}
    />
  );
}
