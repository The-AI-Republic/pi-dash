// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Estimates tab body (PSET-049, EST-026). The settings entry is a thin
// shell: the page gate admits project admins, and the shared estimates
// surface below owns its own loader, empty state, rows and enable
// switch. The tab owns the document title so it tracks the project.

import * as React from "react";

import { useDocumentHead } from "../../../shared/shell/document.js";
import { EstimatesScreen } from "../../estimates/index.js";
import { useProjectSettings } from "../components/ProjectSettingsLayout.js";
import { isAdminStanding } from "../tabs.js";

/** Tab title while the project read is unresolved (EST-026). */
export const ESTIMATES_TAB_TITLE = "Estimates";

/** Browser tab title: the project name followed by the area (EST-026). */
export function estimatesTabTitle(projectName: string | null): string {
  return projectName === null ? ESTIMATES_TAB_TITLE : `${projectName} - ${ESTIMATES_TAB_TITLE}`;
}

export function EstimatesTab(): React.ReactElement {
  const { workspaceSlug, projectId, role, project } = useProjectSettings();
  useDocumentHead({ title: estimatesTabTitle(project?.name ?? null), robots: "noindex, nofollow" });
  return <EstimatesScreen workspaceSlug={workspaceSlug} projectId={projectId} canManage={isAdminStanding(role)} />;
}
