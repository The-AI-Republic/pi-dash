// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake feature tab (PSET-001/PSET-004/PSET-045) — one switch on the
// project's own boolean, read from the already-loaded row
// (parity/specs/project-settings-part-f.md).

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import type { ProjectFeatureSettings as ProjectFeatureSettingsComponent } from "../../../../../../../features/project-features/components/ProjectFeatureSettings.js";
import { PROJECT_FEATURES } from "../../../../../../../features/project-features/lib/registry.js";
import type { ProjectSettingsScreen as ProjectSettingsScreenComponent } from "../../../../../../../features/project-settings/components/ProjectSettingsScreen.js";

// Deferred (size gate): the eager route definition carries the address and
// the feature descriptor; the settings shell and its switch ride a chunk.
const ProjectFeatureSettings = React.lazy(() =>
  import("../../../../../../../features/project-features/components/ProjectFeatureSettings.js").then((module) => ({
    default: module.ProjectFeatureSettings,
  }))
) as typeof ProjectFeatureSettingsComponent;
const ProjectSettingsScreen = React.lazy(() =>
  import("../../../../../../../features/project-settings/components/ProjectSettingsScreen.js").then((module) => ({
    default: module.ProjectSettingsScreen,
  }))
) as typeof ProjectSettingsScreenComponent;

export const Route = createFileRoute("/$ws/_gated/settings/projects/$projectId/features/intake")({
  component: SettingsRouteComponent,
});

function SettingsRouteComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  const feature = PROJECT_FEATURES.intake;
  return (
    <React.Suspense fallback={null}>
      <ProjectSettingsScreen workspaceSlug={ws} projectId={projectId} tabKey="intake">
        <ProjectFeatureSettings feature={feature} />
      </ProjectSettingsScreen>
    </React.Suspense>
  );
}
