// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// States tab leaf (PSET-004/PSET-028–035): the member gate plus the
// workflow-state screen (parity/specs/project-settings-states.md).

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { ProjectSettingsScreen } from "../../../../../../features/project-settings/components/ProjectSettingsScreen.js";
import { StatesTab } from "../../../../../../features/project-states/components/StatesTab.js";

export const Route = createFileRoute("/$ws/_gated/settings/projects/$projectId/states")({
  component: SettingsRouteComponent,
});

function SettingsRouteComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  return (
    <ProjectSettingsScreen workspaceSlug={ws} projectId={projectId} tabKey="states">
      <StatesTab />
    </ProjectSettingsScreen>
  );
}
