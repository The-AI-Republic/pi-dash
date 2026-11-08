/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import { useParams } from "react-router";
import { EUserPermissions, EUserPermissionsLevel } from "@pi-dash/constants";
import { useTranslation } from "@pi-dash/i18n";
import { NotAuthorizedView } from "@/components/auth-screens/not-authorized-view";
import { PageHead } from "@/components/core/page-title";
import { SettingsContentWrapper } from "@/components/settings/content-wrapper";
import { AiWorkersSettingsHeader } from "./header";
import { AiWorkersPanel } from "./panel";

const AiWorkersSettingsPage = observer(function AiWorkersSettingsPage() {
  const { workspaceSlug, projectId } = useParams<{ workspaceSlug: string; projectId: string }>();
  const slug = workspaceSlug ?? "";
  const project = projectId ?? "";
  const { t } = useTranslation();

  // Project settings are admin-only for editing; the panel itself gates the
  // request. Keep the page aligned with schedulers/automations.
  return (
    <SettingsContentWrapper header={<AiWorkersSettingsHeader />}>
      <PageHead title={t("AI Workers")} />
      <AiWorkersPanel workspaceSlug={slug} projectId={project} />
    </SettingsContentWrapper>
  );
});

export default AiWorkersSettingsPage;
