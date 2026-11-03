// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One scheduler install's detail page (parity/specs/project-settings-part-f.md
// §PSET-059; parity/specs/agents.md §3 AGT-013). The address lives under the
// project's schedulers section, which is why the settings tab's rows
// deep-link here; the body owns the configuration, the composed prompt and
// the polled run history.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { BindingDetailPage } from "../../../../../../features/schedulers/BindingDetailPage.js";

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/schedulers/$bindingId")({
  component: ProjectSchedulerBindingComponent,
});

function ProjectSchedulerBindingComponent(): React.ReactElement {
  const { ws, projectId, bindingId } = Route.useParams();
  return <BindingDetailPage workspaceSlug={ws} projectId={projectId} bindingId={bindingId} />;
}
