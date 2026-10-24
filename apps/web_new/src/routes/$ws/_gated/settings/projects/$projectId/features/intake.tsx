// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake feature tab (PSET-001/PSET-004/PSET-045) — one switch on the
// project's own boolean, read from the already-loaded row
// (parity/specs/project-settings-part-f.md).

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { ProjectFeatureSettings } from "../../../../../../../features/project-features/components/ProjectFeatureSettings.js";
import { PROJECT_FEATURES } from "../../../../../../../features/project-features/lib/registry.js";
import { ProjectSettingsScreen } from "../../../../../../../features/project-settings/components/ProjectSettingsScreen.js";

export const Route = createFileRoute("/$ws/_gated/settings/projects/$projectId/features/intake")({
  component: SettingsRouteComponent,
});

function SettingsRouteComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  const feature = PROJECT_FEATURES.intake;
  return (
    <ProjectSettingsScreen workspaceSlug={ws} projectId={projectId} tabKey="intake">
      <ProjectFeatureSettings feature={feature} />
    </ProjectSettingsScreen>
  );
}
