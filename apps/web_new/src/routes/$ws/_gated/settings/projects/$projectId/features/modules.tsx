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

import { ProjectFeatureSettings } from "../../../../../../../features/project-features/components/ProjectFeatureSettings.js";
import { PROJECT_FEATURES } from "../../../../../../../features/project-features/lib/registry.js";
import { ProjectSettingsScreen } from "../../../../../../../features/project-settings/components/ProjectSettingsScreen.js";

export const Route = createFileRoute("/$ws/_gated/settings/projects/$projectId/features/modules")({
  component: SettingsRouteComponent,
});

function SettingsRouteComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  const feature = PROJECT_FEATURES.modules;
  return (
    <ProjectSettingsScreen workspaceSlug={ws} projectId={projectId} tabKey="modules">
      <ProjectFeatureSettings feature={feature} />
    </ProjectSettingsScreen>
  );
}
