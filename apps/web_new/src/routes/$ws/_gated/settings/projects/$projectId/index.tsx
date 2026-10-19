// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// General tab leaf (PSET-001/PSET-005): the shell's landing tab. The
// full form and danger zone belong to the General slice; this body keeps
// the address deep-linkable and the landing tab resolvable.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { ProjectSettingsScreen } from "../../../../../../features/project-settings/index.js";
import { ProjectSettingsGeneralMount } from "../../../../../../features/project-settings/components/ProjectSettingsGeneralMount.js";

export const Route = createFileRoute("/$ws/_gated/settings/projects/$projectId/")({
  component: SettingsRouteComponent,
});

function SettingsRouteComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  return (
    <ProjectSettingsScreen workspaceSlug={ws} projectId={projectId} tabKey="general">
      <ProjectSettingsGeneralMount />
    </ProjectSettingsScreen>
  );
}
