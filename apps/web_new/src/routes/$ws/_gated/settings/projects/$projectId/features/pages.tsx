// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pages feature leaf (PSET-004/PSET-047): admin gate. The switch belongs
// to the features slice.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import {
  ProjectSettingsScreen,
  ProjectSettingsTabMount,
} from "../../../../../../../features/project-settings/index.js";

export const Route = createFileRoute("/$ws/_gated/settings/projects/$projectId/features/pages")({
  component: SettingsRouteComponent,
});

function SettingsRouteComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  return (
    <ProjectSettingsScreen workspaceSlug={ws} projectId={projectId} tabKey="pages">
      <ProjectSettingsTabMount title="Pages" owner="NEWFRONT-582" />
    </ProjectSettingsScreen>
  );
}
