/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
// pi dash imports
import { useTranslation } from "@pi-dash/i18n";
// components
import { EUserPermissions, EUserPermissionsLevel } from "@pi-dash/constants";
import { NotAuthorizedView } from "@/components/auth-screens/not-authorized-view";
import { PageHead } from "@/components/core/page-title";
import { SettingsContentWrapper } from "@/components/settings/content-wrapper";
import { SettingsHeading } from "@/components/settings/heading";
import { ProjectSettingsFeatureControlItem } from "@/components/settings/project/content/feature-control-item";
// hooks
import { useProject } from "@/hooks/store/use-project";
import { useUserPermissions } from "@/hooks/store/user";
// local imports
import type { Route } from "./+types/page";
import { FeaturesPagesProjectSettingsHeader } from "./header";

function FeaturesPagesSettingsPage({ params }: Route.ComponentProps) {
  const { workspaceSlug, projectId } = params;
  // store hooks
  const { workspaceUserInfo, allowPermissions } = useUserPermissions();
  const { currentProjectDetails } = useProject();
  // translation
  const { t } = useTranslation();
  // derived values
  const pageTitle = currentProjectDetails?.name
    ? `${currentProjectDetails?.name} settings - ${t("Pages")}`
    : undefined;
  const canPerformProjectAdminActions = allowPermissions([EUserPermissions.ADMIN], EUserPermissionsLevel.PROJECT);

  if (workspaceUserInfo && !canPerformProjectAdminActions) {
    return <NotAuthorizedView section="settings" isProjectView className="h-auto" />;
  }

  return (
    <SettingsContentWrapper header={<FeaturesPagesProjectSettingsHeader />}>
      <PageHead title={pageTitle} />
      <section className="w-full">
        <SettingsHeading
          title={t("Pages")}
          description={t("Create and edit free-form content; notes, docs, anything.")}
        />
        <div className="mt-7">
          <ProjectSettingsFeatureControlItem
            title={t("Enable pages")}
            description={t("Project members will be able to create and edit pages.")}
            featureProperty="page_view"
            projectId={projectId}
            value={!!currentProjectDetails?.page_view}
            workspaceSlug={workspaceSlug}
          />
        </div>
      </section>
    </SettingsContentWrapper>
  );
}

export default observer(FeaturesPagesSettingsPage);
