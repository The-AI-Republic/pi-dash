/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
// pi dash imports
import { EUserPermissionsLevel } from "@pi-dash/constants";
import { useTranslation } from "@pi-dash/i18n";
import { EmptyStateDetailed } from "@pi-dash/propel/empty-state";
import { EIssuesStoreType, EUserWorkspaceRoles } from "@pi-dash/types";
// hooks
import { useCommandPalette } from "@/hooks/store/use-command-palette";
import { useProject } from "@/hooks/store/use-project";
import { useUserPermissions } from "@/hooks/store/user";

export const GlobalViewEmptyState = observer(function GlobalViewEmptyState() {
  // pi dash imports
  const { t } = useTranslation();
  // store hooks
  const { workspaceProjectIds } = useProject();
  const { toggleCreateIssueModal, toggleCreateProjectModal } = useCommandPalette();
  const { allowPermissions } = useUserPermissions();
  // derived values
  const hasMemberLevelPermission = allowPermissions(
    [EUserWorkspaceRoles.ADMIN, EUserWorkspaceRoles.MEMBER],
    EUserPermissionsLevel.WORKSPACE
  );

  if (workspaceProjectIds?.length === 0) {
    return (
      <EmptyStateDetailed
        title={t("No project")}
        description={t("To create work items or manage your work, you need to create a project or be a part of one.")}
        assetKey="project"
        assetClassName="size-40"
        actions={[
          {
            label: t("Start your first project"),
            onClick: () => {
              toggleCreateProjectModal(true);
            },
            disabled: !hasMemberLevelPermission,
            variant: "primary",
          },
        ]}
      />
    );
  }

  return (
    <EmptyStateDetailed
      title={t("No Views yet")}
      description={t("Add work items to your project and use views to filter, sort, and monitor progress effortlessly.")}
      assetKey="project"
      assetClassName="size-40"
      actions={[
        {
          label: t("Add work item"),
          onClick: () => {
            toggleCreateIssueModal(true, EIssuesStoreType.PROJECT);
          },
          disabled: !hasMemberLevelPermission,
          variant: "primary",
        },
      ]}
    />
  );
});
