// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project automations settings (parity/specs/agents.md §8, AGT-034–037).
// Thin composition: the automations feature owns the screen, its admin
// gate and the extension slot; the settings shell owns the rail and the
// active marker. The community build contributes no extensions, so the
// slot renders empty.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { AutomationsSettings } from "../../../../../../features/automations/index.js";
import { ProjectSettingsScreen } from "../../../../../../features/project-settings/index.js";

export const Route = createFileRoute("/$ws/_gated/settings/projects/$projectId/automations")({
  component: AutomationsRouteComponent,
});

function AutomationsRouteComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  return (
    <ProjectSettingsScreen workspaceSlug={ws} projectId={projectId} tabKey="automations" pageHeader={false}>
      <div className="mx-auto w-full max-w-3xl px-(--space-6) py-(--space-8)">
        <AutomationsSettings workspaceSlug={ws} projectId={projectId} />
      </div>
    </ProjectSettingsScreen>
  );
}
