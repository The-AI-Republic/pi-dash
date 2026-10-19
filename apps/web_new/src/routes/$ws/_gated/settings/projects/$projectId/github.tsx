// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Repository tab leaf (PSET-004/PSET-050–052): the page gate admits
// members although the sidebar hides the entry. The sync card belongs to
// the estimates+github slice.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { ProjectSettingsScreen, ProjectSettingsTabMount } from "../../../../../../features/project-settings/index.js";

export const Route = createFileRoute("/$ws/_gated/settings/projects/$projectId/github")({
  component: SettingsRouteComponent,
});

function SettingsRouteComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  return (
    <ProjectSettingsScreen workspaceSlug={ws} projectId={projectId} tabKey="github">
      <ProjectSettingsTabMount title="Repository" owner="NEWFRONT-602" />
    </ProjectSettingsScreen>
  );
}
