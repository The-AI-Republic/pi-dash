// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Modules feature tab (PSET-001/PSET-004/PSET-046, MOD-036) — the
// modules area's own body over the settings screen: an admin sees the one
// switch for the project's modules flag, everyone else a refusal in its
// place. The switch reads the already-loaded project row
// (parity/specs/modules.md §10.1).

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { ModuleFeatureSettings } from "../../../../../../../features/modules/index.js";
import { ProjectSettingsScreen } from "../../../../../../../features/project-settings/index.js";

export const Route = createFileRoute("/$ws/_gated/settings/projects/$projectId/features/modules")({
  component: SettingsRouteComponent,
});

function SettingsRouteComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  return (
    <ProjectSettingsScreen workspaceSlug={ws} projectId={projectId} tabKey="modules">
      <ModuleFeatureSettings workspaceSlug={ws} projectId={projectId} />
    </ProjectSettingsScreen>
  );
}
