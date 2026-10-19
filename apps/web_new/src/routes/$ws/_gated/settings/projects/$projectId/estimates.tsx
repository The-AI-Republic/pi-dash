// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Estimates tab leaf (PSET-004/PSET-049): admin gate. The estimates entry
// belongs to the estimates+github slice.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { ProjectSettingsScreen, ProjectSettingsTabMount } from "../../../../../../features/project-settings/index.js";

export const Route = createFileRoute("/$ws/_gated/settings/projects/$projectId/estimates")({
  component: SettingsRouteComponent,
});

function SettingsRouteComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  return (
    <ProjectSettingsScreen workspaceSlug={ws} projectId={projectId} tabKey="estimates">
      <ProjectSettingsTabMount title="Estimates" owner="NEWFRONT-602" />
    </ProjectSettingsScreen>
  );
}
