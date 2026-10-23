// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project automations settings (parity/specs/agents.md §8, AGT-034–037;
// parity/specs/project-settings-part-f.md, PSET-061). Thin composition: the
// automations feature owns the screen, its admin gate and the extension
// slot; the settings shell owns the rail and the active marker. The shell
// starts on the automations skeleton while the project read is unresolved,
// so the loader is reachable instead of settling straight on rows. The
// community build contributes no extensions, so the slot renders empty.
// The automations screen owns its refusal (automations-not-authorized,
// PSET-053/054), so the route asks the shell for the body-owned gate
// rather than the generic tab denial.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { AutomationsSettings, AutomationsSkeleton } from "../../../../../../features/automations/index.js";
import { ProjectSettingsScreen } from "../../../../../../features/project-settings/index.js";

export const Route = createFileRoute("/$ws/_gated/settings/projects/$projectId/automations")({
  component: AutomationsRouteComponent,
});

function AutomationsRouteComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  return (
    <ProjectSettingsScreen
      workspaceSlug={ws}
      projectId={projectId}
      tabKey="automations"
      gate="body"
      pendingFallback={<AutomationsSkeleton />}
    >
      <AutomationsSettings workspaceSlug={ws} projectId={projectId} />
    </ProjectSettingsScreen>
  );
}
