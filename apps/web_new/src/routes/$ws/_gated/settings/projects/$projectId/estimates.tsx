// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Estimates tab leaf (PSET-004/PSET-049): the admin gate, then the shared
// estimates surface with its loader, empty state and enable switch.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { EstimatesTab } from "../../../../../../features/project-settings/estimates/EstimatesTab.js";
import { ProjectSettingsScreen } from "../../../../../../features/project-settings/components/ProjectSettingsScreen.js";

export const Route = createFileRoute("/$ws/_gated/settings/projects/$projectId/estimates")({
  component: SettingsRouteComponent,
});

function SettingsRouteComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  return (
    <ProjectSettingsScreen workspaceSlug={ws} projectId={projectId} tabKey="estimates">
      <EstimatesTab />
    </ProjectSettingsScreen>
  );
}
