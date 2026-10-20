// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Members tab leaf (PSET-004/PSET-018–021, PSET-027): the page gate is
// stricter than the sidebar for guests, and the members feature renders
// the roster read side inside it.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { ProjectSettingsScreen } from "../../../../../../features/project-settings/index.js";
import { MembersSettings } from "../../../../../../features/project-settings/members/index.js";

export const Route = createFileRoute("/$ws/_gated/settings/projects/$projectId/members")({
  component: SettingsRouteComponent,
});

function SettingsRouteComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  return (
    <ProjectSettingsScreen workspaceSlug={ws} projectId={projectId} tabKey="members">
      <MembersSettings workspaceSlug={ws} projectId={projectId} />
    </ProjectSettingsScreen>
  );
}
