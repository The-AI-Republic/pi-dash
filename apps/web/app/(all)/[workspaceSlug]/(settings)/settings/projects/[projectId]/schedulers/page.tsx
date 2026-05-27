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
import { SchedulerBindingsPanel } from "@/components/project/scheduler-bindings/bindings-panel";
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

  // Project-settings UX is admin-only: keeps the legacy behavior intact so
  // members aren't routed here from the settings sidebar. The matching
  // project-level page at /projects/:id/schedulers is what surfaces a
  // read-only view to non-admins.
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
      <SchedulerBindingsPanel workspaceSlug={slug} projectId={project} />
    </SettingsContentWrapper>
  );
});

export default SchedulerBindingsSettingsPage;
