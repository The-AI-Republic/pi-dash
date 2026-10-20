// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pages feature tab (PSET-001/PSET-004/PSET-047) — one switch on the
// project's own boolean, read from the already-loaded row
// (parity/specs/project-settings-part-f.md).

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { ProjectFeatureSettings, findProjectFeature } from "../../../../../../../features/project-features/index.js";
import { ProjectSettingsScreen } from "../../../../../../../features/project-settings/index.js";

export const Route = createFileRoute("/$ws/_gated/settings/projects/$projectId/features/pages")({
  component: SettingsRouteComponent,
});

function SettingsRouteComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  const feature = findProjectFeature("pages");
  if (feature === undefined) throw new Error("Unknown project feature: pages");
  return (
    <ProjectSettingsScreen workspaceSlug={ws} projectId={projectId} tabKey="pages">
      <ProjectFeatureSettings feature={feature} />
    </ProjectSettingsScreen>
  );
}
