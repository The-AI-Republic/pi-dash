/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
// pi dash imports
import { EUserPermissionsLevel, EUserPermissions } from "@pi-dash/constants";
import { useTranslation } from "@pi-dash/i18n";
import { EmptyStateDetailed } from "@pi-dash/propel/empty-state";
import { ContentWrapper } from "@pi-dash/ui";
// components
import { calculateTotalFilters } from "@pi-dash/utils";
import { ProjectsLoader } from "@/components/ui/loader/projects-loader";
// hooks
import { useCommandPalette } from "@/hooks/store/use-command-palette";
import { useProject } from "@/hooks/store/use-project";
import { useProjectFilter } from "@/hooks/store/use-project-filter";
import { useUserPermissions } from "@/hooks/store/user";
// local imports
import { ProjectCard } from "./card";

type TProjectCardListProps = {
  totalProjectIds?: string[];
  filteredProjectIds?: string[];
};

export const ProjectCardList = observer(function ProjectCardList(props: TProjectCardListProps) {
  const { totalProjectIds: totalProjectIdsProps, filteredProjectIds: filteredProjectIdsProps } = props;
  // pi dash hooks
  const { t } = useTranslation();
  // store hooks
  const { toggleCreateProjectModal } = useCommandPalette();
  const {
    loader,
    fetchStatus,
    workspaceProjectIds: storeWorkspaceProjectIds,
    filteredProjectIds: storeFilteredProjectIds,
    getProjectById,
  } = useProject();
  const { currentWorkspaceDisplayFilters, currentWorkspaceFilters } = useProjectFilter();
  const { allowPermissions } = useUserPermissions();

  // derived values
  const workspaceProjectIds = totalProjectIdsProps ?? storeWorkspaceProjectIds;
  const filteredProjectIds = filteredProjectIdsProps ?? storeFilteredProjectIds;

  // permissions
  const canPerformEmptyStateActions = allowPermissions(
    [EUserPermissions.ADMIN, EUserPermissions.MEMBER],
    EUserPermissionsLevel.WORKSPACE
  );

  if (!filteredProjectIds || !workspaceProjectIds || loader === "init-loader" || fetchStatus !== "complete")
    return <ProjectsLoader />;

  if (workspaceProjectIds?.length === 0 && !currentWorkspaceDisplayFilters?.archived_projects)
    return (
      <EmptyStateDetailed
        title={t("No active projects")}
        description={t("Think of each project as the parent for goal-oriented work. Projects are where Jobs, Cycles, and Modules live and, along with your colleagues, help you achieve that goal. Create a new project or filter for archived projects.")}
        assetKey="project"
        assetClassName="size-40"
        actions={[
          {
            label: t("Start your first project"),
            onClick: () => {
              toggleCreateProjectModal(true);
            },
            disabled: !canPerformEmptyStateActions,
            variant: "primary",
          },
        ]}
      />
    );

  if (filteredProjectIds.length === 0)
    return (
      <EmptyStateDetailed
        title={
          currentWorkspaceDisplayFilters?.archived_projects &&
          calculateTotalFilters(currentWorkspaceFilters ?? {}) === 0
            ? t("No projects archived")
            : t("No matching results.")
        }
        description={
          currentWorkspaceDisplayFilters?.archived_projects &&
          calculateTotalFilters(currentWorkspaceFilters ?? {}) === 0
            ? t("Looks like all your projects are still active—great job!")
            : t("No results found. Try adjusting your search terms.")
        }
        assetKey={
          currentWorkspaceDisplayFilters?.archived_projects &&
          calculateTotalFilters(currentWorkspaceFilters ?? {}) === 0
            ? "archived-work-item"
            : "search"
        }
        assetClassName="size-40"
      />
    );

  return (
    <ContentWrapper>
      <div className="grid grid-cols-1 gap-8 md:grid-cols-2 lg:grid-cols-3">
        {filteredProjectIds.map((projectId) => {
          const projectDetails = getProjectById(projectId);
          if (!projectDetails) return;
          return <ProjectCard key={projectDetails.id} project={projectDetails} />;
        })}
      </div>
    </ContentWrapper>
  );
});
