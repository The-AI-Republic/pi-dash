// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Bare project address redirect (parity/specs/shell.md §7, SHELL-077).
// The address rewrites to the stored default tab, or to tracking when
// the stored key is unknown, gated away, or unreadable.

import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { useProjectRoles } from "../../auth/index.js";
import { parseNavigationPrefs, selectDefaultTarget, selectProjectTabs, tabHref } from "../tabs.js";
import { projectsQueryOptions } from "./../api/queries.js";
import { useProjectProperties } from "./../api/tabPreferences.js";

export interface DefaultTabRedirectProps {
  workspaceSlug: string;
  projectId: string;
}

export function DefaultTabRedirect({ workspaceSlug, projectId }: DefaultTabRedirectProps): React.ReactElement | null {
  const navigate = useNavigate();
  const projects = useQuery(projectsQueryOptions(getAppClient(), workspaceSlug));
  const stored = useProjectProperties(workspaceSlug, projectId);
  const roles = useProjectRoles(workspaceSlug);

  const pending = [projects, stored, roles].some((query) => query.isPending);
  const project = projects.data?.find((entry) => entry.id === projectId) ?? null;
  const role = roles.data?.[projectId] ?? null;
  const tabs = project === null ? [] : selectProjectTabs({ workspaceSlug, projectId, flags: project, role });
  const target =
    selectDefaultTarget(tabs, parseNavigationPrefs(stored.data?.preferences).defaultTab)?.href ??
    tabHref(workspaceSlug, projectId, "issues");

  React.useEffect(() => {
    if (!pending) navigate({ to: target, replace: true });
  }, [pending, target, navigate]);

  return null;
}
