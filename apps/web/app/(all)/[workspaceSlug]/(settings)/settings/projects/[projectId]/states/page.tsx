/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import { EUserPermissions, EUserPermissionsLevel } from "@apple-pi-dash/constants";
import { useTranslation } from "@apple-pi-dash/i18n";
import { setPromiseToast } from "@apple-pi-dash/propel/toast";
import { ToggleSwitch } from "@apple-pi-dash/ui";
// components
import { NotAuthorizedView } from "@/components/auth-screens/not-authorized-view";
import { PageHead } from "@/components/core/page-title";
import { ProjectStateRoot } from "@/components/project-states";
import { SettingsContentWrapper } from "@/components/settings/content-wrapper";
import { SettingsBoxedControlItem } from "@/components/settings/boxed-control-item";
import { SettingsHeading } from "@/components/settings/heading";
// hook
import { useProject } from "@/hooks/store/use-project";
import { useUserPermissions } from "@/hooks/store/user";
// local imports
import type { Route } from "./+types/page";
import { StatesProjectSettingsHeader } from "./header";

function StatesSettingsPage({ params }: Route.ComponentProps) {
  const { workspaceSlug, projectId } = params;
  // store
  const { currentProjectDetails, getProjectById, updateProject } = useProject();
  const { workspaceUserInfo, allowPermissions } = useUserPermissions();

  const { t } = useTranslation();

  // derived values
  const pageTitle = currentProjectDetails?.name ? `${currentProjectDetails?.name} - States` : undefined;
  const canPerformProjectMemberActions = allowPermissions(
    [EUserPermissions.ADMIN, EUserPermissions.MEMBER],
    EUserPermissionsLevel.PROJECT
  );
  const isAdmin = allowPermissions(
    [EUserPermissions.ADMIN],
    EUserPermissionsLevel.PROJECT,
    workspaceSlug,
    projectId
  );
  const membersCanEditStates = getProjectById(projectId)?.members_can_edit_states ?? true;

  const handleMembersCanEditStatesToggle = () => {
    if (!workspaceSlug || !projectId) return;
    const promise = updateProject(workspaceSlug, projectId, {
      members_can_edit_states: !membersCanEditStates,
    });
    setPromiseToast(promise, {
      loading: t("project_settings.states.members_edit.toast.loading"),
      success: {
        title: t("project_settings.states.members_edit.toast.success_title"),
        message: () => t("project_settings.states.members_edit.toast.success_message"),
      },
      error: {
        title: t("project_settings.states.members_edit.toast.error_title"),
        message: () => t("project_settings.states.members_edit.toast.error_message"),
      },
    });
  };

  if (workspaceUserInfo && !canPerformProjectMemberActions) {
    return <NotAuthorizedView section="settings" isProjectView className="h-auto" />;
  }

  return (
    <SettingsContentWrapper header={<StatesProjectSettingsHeader />}>
      <PageHead title={pageTitle} />
      <div className="w-full">
        <SettingsHeading
          title={t("project_settings.states.heading")}
          description={t("project_settings.states.description")}
        />
        {isAdmin && (
          <div className="mt-6">
            <SettingsBoxedControlItem
              title={t("project_settings.states.members_edit.title")}
              description={t("project_settings.states.members_edit.description")}
              control={
                <ToggleSwitch
                  value={membersCanEditStates}
                  onChange={handleMembersCanEditStatesToggle}
                  size="sm"
                />
              }
            />
          </div>
        )}
        <div className="mt-6">
          <ProjectStateRoot workspaceSlug={workspaceSlug} projectId={projectId} />
        </div>
      </div>
    </SettingsContentWrapper>
  );
}

export default observer(StatesSettingsPage);
