// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Members tab leaf (PSET-004/PSET-018–021): the page gate is stricter than
// the sidebar for guests. The table belongs to the members slice.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { ProjectSettingsScreen, ProjectSettingsTabMount } from "../../../../../../features/project-settings/index.js";

export const Route = createFileRoute("/$ws/_gated/settings/projects/$projectId/members")({
  component: SettingsRouteComponent,
});

function SettingsRouteComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  return (
    <ProjectSettingsScreen workspaceSlug={ws} projectId={projectId} tabKey="members">
      <ProjectSettingsTabMount title="Members" owner="NEWFRONT-574" />
    </ProjectSettingsScreen>
  );
}
