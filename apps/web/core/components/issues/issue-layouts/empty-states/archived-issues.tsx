/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import { useParams } from "next/navigation";
// pi dash imports
import { EUserPermissionsLevel } from "@pi-dash/constants";
import { useTranslation } from "@pi-dash/i18n";
import { EmptyStateDetailed } from "@pi-dash/propel/empty-state";
import { EIssuesStoreType, EUserProjectRoles } from "@pi-dash/types";
// hooks
import { useUserPermissions } from "@/hooks/store/user";
import { useWorkItemFilterInstance } from "@/hooks/store/work-item-filters/use-work-item-filter-instance";
import { useAppRouter } from "@/hooks/use-app-router";

export const ProjectArchivedEmptyState = observer(function ProjectArchivedEmptyState() {
  // router
  const router = useAppRouter();
  const { workspaceSlug: routerWorkspaceSlug, projectId: routerProjectId } = useParams();
  const workspaceSlug = routerWorkspaceSlug ? routerWorkspaceSlug.toString() : undefined;
  const projectId = routerProjectId ? routerProjectId.toString() : undefined;
  // pi dash hooks
  const { t } = useTranslation();
  // store hooks
  const { allowPermissions } = useUserPermissions();
  // derived values
  const archivedWorkItemFilter = useWorkItemFilterInstance(EIssuesStoreType.ARCHIVED, projectId);
  const canPerformEmptyStateActions = allowPermissions(
    [EUserProjectRoles.ADMIN, EUserProjectRoles.MEMBER],
    EUserPermissionsLevel.PROJECT
  );

  return (
    <div className="relative h-full w-full overflow-y-auto">
      {archivedWorkItemFilter?.hasActiveFilters ? (
        <EmptyStateDetailed
          assetKey="search"
          title={t("No matching results.")}
          description={t("No results found. Try adjusting your search terms.")}
          actions={[
            {
              label: "Clear filters",
              onClick: archivedWorkItemFilter?.clearFilters,
              disabled: !canPerformEmptyStateActions || !archivedWorkItemFilter,
              variant: "secondary",
            },
          ]}
        />
      ) : (
        <EmptyStateDetailed
          assetKey="archived-work-item"
          title={t("No archived work items yet")}
          description={t("Manually or through automation, you can archive work items that are completed or cancelled. Find them here once archived.")}
          actions={[
            {
              label: t("Set automation"),
              onClick: () => router.push(`/${workspaceSlug}/settings/projects/${projectId}/automations`),
              disabled: !canPerformEmptyStateActions,
              variant: "primary",
            },
          ]}
        />
      )}
    </div>
  );
});
