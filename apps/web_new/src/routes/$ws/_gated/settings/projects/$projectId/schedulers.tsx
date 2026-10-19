// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Schedulers tab leaf (PSET-004/PSET-055–061): admin gate. The installs
// table belongs to the schedulers slice.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { ProjectSettingsScreen, ProjectSettingsTabMount } from "../../../../../../features/project-settings/index.js";

export const Route = createFileRoute("/$ws/_gated/settings/projects/$projectId/schedulers")({
  component: SettingsRouteComponent,
});

function SettingsRouteComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  return (
    <ProjectSettingsScreen workspaceSlug={ws} projectId={projectId} tabKey="schedulers">
      <ProjectSettingsTabMount title="Schedulers" owner="NEWFRONT-604" />
    </ProjectSettingsScreen>
  );
}
