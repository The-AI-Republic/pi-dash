/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
// pi dash imports
import { EUserPermissionsLevel } from "@pi-dash/constants";
import { useTranslation } from "@pi-dash/i18n";
import { EUserProjectRoles } from "@pi-dash/types";
// components
import { NotAuthorizedView } from "@/components/auth-screens/not-authorized-view";
import { PageHead } from "@/components/core/page-title";
import { ProjectSchedulerBindingsList } from "@/components/project/scheduler-bindings/binding-list";
// hooks
import { useProject } from "@/hooks/store/use-project";
import { useUserPermissions } from "@/hooks/store/user";
import { useAppRouter } from "@/hooks/use-app-router";
import type { Route } from "./+types/page";

function ProjectSchedulersPage({ params }: Route.ComponentProps) {
  const { workspaceSlug, projectId } = params;
  const router = useAppRouter();
  const { t } = useTranslation();
  const { currentProjectDetails } = useProject();
  const { workspaceUserInfo, allowPermissions } = useUserPermissions();

  // Project admin only — matches the existing settings → schedulers gate
  // and the backend's project-admin gate on binding mutations.
  const canManage = allowPermissions(
    [EUserProjectRoles.ADMIN],
    EUserPermissionsLevel.PROJECT,
    workspaceSlug,
    projectId
  );

  const pageTitle = currentProjectDetails?.name
    ? `${currentProjectDetails.name} · ${t("scheduler_bindings.title")}`
    : t("scheduler_bindings.title");

  // Feature disabled at the project level — bounce admins to the
  // features settings page so they can flip the toggle back on.
  if (currentProjectDetails && currentProjectDetails.scheduler_view === false) {
    return (
      <div className="flex h-full w-full items-center justify-center">
        <div className="max-w-md p-6 text-center">
          <h1 className="text-16 font-semibold text-primary">{t("scheduler_bindings.title")}</h1>
          <p className="mt-2 text-13 text-secondary">{t("scheduler_bindings.subtitle")}</p>
          <button
            type="button"
            className="mt-4 text-13 font-medium text-primary underline"
            onClick={() => router.push(`/${workspaceSlug}/settings/projects/${projectId}/`)}
            disabled={!canManage}
          >
            {t("settings")}
          </button>
        </div>
      </div>
    );
  }

  if (workspaceUserInfo && !canManage) {
    return <NotAuthorizedView section="settings" isProjectView className="h-auto" />;
  }

  return (
    <div className="flex h-full flex-col">
      <PageHead title={pageTitle} />
      <div className="h-full w-full overflow-y-auto p-6">
        <ProjectSchedulerBindingsList workspaceSlug={workspaceSlug} projectId={projectId} />
      </div>
    </div>
  );
}

export default observer(ProjectSchedulersPage);
