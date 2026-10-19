// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// States tab leaf (PSET-004/PSET-028–035): member gate. The state tree
// belongs to the states slice.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { ProjectSettingsScreen, ProjectSettingsTabMount } from "../../../../../../features/project-settings/index.js";

export const Route = createFileRoute("/$ws/_gated/settings/projects/$projectId/states")({
  component: SettingsRouteComponent,
});

function SettingsRouteComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  return (
    <ProjectSettingsScreen workspaceSlug={ws} projectId={projectId} tabKey="states">
      <ProjectSettingsTabMount title="States" owner="NEWFRONT-598" />
    </ProjectSettingsScreen>
  );
}
