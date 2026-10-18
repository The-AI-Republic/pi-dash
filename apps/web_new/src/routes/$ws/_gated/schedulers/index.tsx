// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace scheduler catalog (parity/specs/agents.md §1, AGT-001–005).
// The route stays thin: it prefetches the workspace-scoped definition
// list and mounts the feature-owned connected catalog.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { SchedulerCatalog, schedulersQueryOptions } from "../../../../features/schedulers/index.js";

export const Route = createFileRoute("/$ws/_gated/schedulers/")({
  loader: ({ context, params }) =>
    context.queryClient.ensureQueryData(schedulersQueryOptions(context.apiClient, params.ws)),
  component: SchedulersComponent,
});

function SchedulersComponent(): React.ReactElement {
  const { ws } = Route.useParams();
  return <SchedulerCatalog key={ws} workspaceSlug={ws} />;
}
