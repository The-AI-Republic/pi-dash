/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
// pi dash imports
import { EUserPermissions, EUserPermissionsLevel } from "@pi-dash/constants";
import type { IAppIntegration } from "@pi-dash/types";
// components
import { NotAuthorizedView } from "@/components/auth-screens/not-authorized-view";
import { PageHead } from "@/components/core/page-title";
import { GithubPatCard } from "@/components/integration/github/github-pat-card";
import { IntegrationAndImportExportBanner } from "@/components/ui/integration-and-import-export-banner";
// hooks
import { useWorkspace } from "@/hooks/store/use-workspace";
import { useUserPermissions } from "@/hooks/store/user";

// PR-65 follow-up: the legacy `GET /api/integrations/` endpoint that used to
// drive `IntegrationService.getAppIntegrationsList()` was removed somewhere
// upstream; pi-dash currently only ships the PAT-based GitHub integration.
// Render the GitHub card directly with a stub `integration` prop instead of
// relying on the broken list.
const GITHUB_INTEGRATION_STUB: IAppIntegration = {
  id: "github",
  provider: "github",
  title: "GitHub",
  description: "",
  author: "",
  avatar_url: null,
  redirect_url: "",
  webhook_url: "",
  webhook_secret: "",
  network: 1,
  metadata: {},
  verified: true,
  created_at: "",
  updated_at: "",
  created_by: null,
  updated_by: null,
};

function WorkspaceIntegrationsPage() {
  // store hooks
  const { currentWorkspace } = useWorkspace();
  const { allowPermissions } = useUserPermissions();

  // derived values
  const isAdmin = allowPermissions([EUserPermissions.ADMIN], EUserPermissionsLevel.WORKSPACE);
  const pageTitle = currentWorkspace?.name ? `${currentWorkspace.name} - Integrations` : undefined;

  if (!isAdmin) return <NotAuthorizedView section="settings" className="h-auto" />;

  return (
    <>
      <PageHead title={pageTitle} />
      <section className="w-full overflow-y-auto">
        <IntegrationAndImportExportBanner bannerName="Integrations" />
        <div>
          <GithubPatCard integration={GITHUB_INTEGRATION_STUB} />
        </div>
      </section>
    </>
  );
}

export default observer(WorkspaceIntegrationsPage);
