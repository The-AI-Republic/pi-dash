// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// AI Workers tab leaf (hybrid assignment, design §6.6): the
// lost-runner rule page. Project admins edit; the tab gate admits
// members to the read-only form and refuses guests.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { ProjectSettingsAiWorkersTab } from "../../../../../../features/project-settings/ai-workers/AiWorkersTabScreen.js";
import { ProjectSettingsScreen } from "../../../../../../features/project-settings/components/ProjectSettingsScreen.js";

export const Route = createFileRoute("/$ws/_gated/settings/projects/$projectId/ai-workers")({
  component: SettingsRouteComponent,
});

function SettingsRouteComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  return (
    <ProjectSettingsScreen workspaceSlug={ws} projectId={projectId} tabKey="ai_workers">
      <ProjectSettingsAiWorkersTab workspaceSlug={ws} projectId={projectId} />
    </ProjectSettingsScreen>
  );
}
