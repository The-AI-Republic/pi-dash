// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project labels settings (parity/specs/issues.md §19, ISS-226–230).
// Thin: the labels feature owns the screen; the project-settings shell
// around it owns the rail, the active marker and the role gate. No
// loader prefetch — the list loads client-side so the skeleton renders
// while pending (ISS-230).

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { LabelsSettings } from "../../../../../../features/issues/index.js";
import { ProjectSettingsScreen } from "../../../../../../features/project-settings/index.js";

export const Route = createFileRoute("/$ws/_gated/settings/projects/$projectId/labels")({
  component: LabelsRouteComponent,
});

function LabelsRouteComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  return (
    <ProjectSettingsScreen workspaceSlug={ws} projectId={projectId} tabKey="labels">
      <LabelsSettings workspaceSlug={ws} projectId={projectId} />
    </ProjectSettingsScreen>
  );
}
