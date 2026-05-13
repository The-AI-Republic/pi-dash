/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import { useParams } from "react-router";
// pi dash imports
import { EUserPermissions, EUserPermissionsLevel } from "@pi-dash/constants";
import { useTranslation } from "@pi-dash/i18n";
// components
import { NotAuthorizedView } from "@/components/auth-screens/not-authorized-view";
import { PageHead } from "@/components/core/page-title";
import { ProjectSchedulerBindingsList } from "@/components/project/scheduler-bindings/binding-list";
import { SettingsContentWrapper } from "@/components/settings/content-wrapper";
// hooks
import { useProject } from "@/hooks/store/use-project";
import { useUserPermissions } from "@/hooks/store/user";
import { SchedulersProjectSettingsHeader } from "./header";

const SchedulerBindingsSettingsPage = observer(function SchedulerBindingsSettingsPage() {
  const { workspaceSlug, projectId } = useParams<{ workspaceSlug: string; projectId: string }>();
  const { currentProjectDetails } = useProject();
  const { workspaceUserInfo, allowPermissions } = useUserPermissions();
  const { t } = useTranslation();

  const slug = workspaceSlug ?? "";
  const project = projectId ?? "";

  // Project admin only — matches the route's access list and the
  // backend's project-admin gate on binding mutations. Surfacing the
  // page to non-admins would only show a read-only list with no
  // actions, which adds no value over the workspace catalog view.
  const canManage = allowPermissions([EUserPermissions.ADMIN], EUserPermissionsLevel.PROJECT, slug, project);

  const pageTitle = currentProjectDetails?.name
    ? `${currentProjectDetails.name} · ${t("scheduler_bindings.title")}`
    : t("scheduler_bindings.title");

  if (workspaceUserInfo && !canManage) {
    return <NotAuthorizedView section="settings" isProjectView className="h-auto" />;
  }

  return (
    <SettingsContentWrapper header={<SchedulersProjectSettingsHeader />}>
      <PageHead title={pageTitle} />
      <div className="p-6">
        <ProjectSchedulerBindingsList workspaceSlug={slug} projectId={project} />
      </div>
    </SettingsContentWrapper>
  );
});

export default SchedulerBindingsSettingsPage;
