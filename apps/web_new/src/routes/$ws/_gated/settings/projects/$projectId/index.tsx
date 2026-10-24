// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// General tab leaf (PSET-001/PSET-005): the shell's landing tab. It draws
// the settings screen around the General body, which owns the project form.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { ProjectSettingsGeneralTab } from "../../../../../../features/project-settings/general/GeneralTabScreen.js";
import { ProjectSettingsScreen } from "../../../../../../features/project-settings/components/ProjectSettingsScreen.js";

export const Route = createFileRoute("/$ws/_gated/settings/projects/$projectId/")({
  component: SettingsRouteComponent,
});

function SettingsRouteComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  return (
    <ProjectSettingsScreen workspaceSlug={ws} projectId={projectId} tabKey="general">
      <ProjectSettingsGeneralTab workspaceSlug={ws} projectId={projectId} />
    </ProjectSettingsScreen>
  );
}
