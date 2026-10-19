// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Views feature leaf (PSET-004/PSET-043): admin gate. The switch belongs
// to the features slice.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import {
  ProjectSettingsScreen,
  ProjectSettingsTabMount,
} from "../../../../../../../features/project-settings/index.js";

export const Route = createFileRoute("/$ws/_gated/settings/projects/$projectId/features/views")({
  component: SettingsRouteComponent,
});

function SettingsRouteComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  return (
    <ProjectSettingsScreen workspaceSlug={ws} projectId={projectId} tabKey="views">
      <ProjectSettingsTabMount title="Views" owner="NEWFRONT-582" />
    </ProjectSettingsScreen>
  );
}
