/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import { useParams } from "next/navigation";
// pi dash imports
import { EUserPermissions, EUserPermissionsLevel } from "@pi-dash/constants";
// hooks
import { useUserPermissions } from "@/hooks/store/user";
import { useGlobalViewId } from "@/hooks/use-global-view-id";
// local imports
import { AllIssueQuickActions } from "../../quick-action-dropdowns";
import { BaseListRoot } from "../base-list-root";

// List layout for the workspace-level "all issues" view. Mirrors the project
// list root but is driven by the GLOBAL issues store (set via context by the
// AllIssueLayoutRoot) and uses AllIssueQuickActions since issues span projects.
export const GlobalIssueListLayout = observer(function GlobalIssueListLayout() {
  // router
  const { workspaceSlug } = useParams();
  const globalViewId = useGlobalViewId();
  // hooks
  const { allowPermissions } = useUserPermissions();

  if (!workspaceSlug) return null;

  const canEditPropertiesBasedOnProject = (projectId: string) =>
    allowPermissions(
      [EUserPermissions.ADMIN, EUserPermissions.MEMBER],
      EUserPermissionsLevel.PROJECT,
      workspaceSlug.toString(),
      projectId
    );

  return (
    <BaseListRoot
      QuickActions={AllIssueQuickActions}
      canEditPropertiesBasedOnProject={canEditPropertiesBasedOnProject}
      viewId={globalViewId}
    />
  );
});
