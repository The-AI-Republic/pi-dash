/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import { EUserPermissions, EUserPermissionsLevel } from "@pi-dash/constants";
import { NotAuthorizedView } from "@/components/auth-screens/not-authorized-view";
import { PageHead } from "@/components/core/page-title";
import { ProjectGithubSyncSection } from "@/components/project/settings/github-sync";
import { SettingsContentWrapper } from "@/components/settings/content-wrapper";
import { useProject } from "@/hooks/store/use-project";
import { useUserPermissions } from "@/hooks/store/user";
import { GithubProjectSettingsHeader } from "./header";

function GithubSettingsPage() {
  const { currentProjectDetails } = useProject();
  const { workspaceUserInfo, allowPermissions } = useUserPermissions();

  const pageTitle = currentProjectDetails?.name ? `${currentProjectDetails.name} - GitHub` : undefined;

  const canManage = allowPermissions(
    [EUserPermissions.ADMIN, EUserPermissions.MEMBER],
    EUserPermissionsLevel.PROJECT
  );

  if (workspaceUserInfo && !canManage) {
    return <NotAuthorizedView section="settings" isProjectView className="h-auto" />;
  }

  return (
    <SettingsContentWrapper header={<GithubProjectSettingsHeader />}>
      <PageHead title={pageTitle} />
      <div className="size-full">
        <ProjectGithubSyncSection />
      </div>
    </SettingsContentWrapper>
  );
}

export default observer(GithubSettingsPage);
