// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace scheduler catalog (parity/specs/agents.md §1, AGT-001–005).
// The route stays thin: it prefetches the workspace-scoped definition
// list and mounts the feature-owned connected catalog.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { SchedulerCatalog } from "../../../../features/schedulers/index.js";

export const Route = createFileRoute("/$ws/_gated/schedulers/")({
  // NOTE (NEWFRONT-852, size gate): the definition-list read stays behind
  // a dynamic import so the scheduler api modules never join the initial
  // bundle — this loader only runs on scheduler navigation, after first
  // paint. The loader still resolves before render, as before.
  loader: async ({ context, params }) => {
    const { schedulersQueryOptions } = await import("../../../../features/schedulers/api.js");
    return context.queryClient.ensureQueryData(schedulersQueryOptions(context.apiClient, params.ws));
  },
  component: SchedulersComponent,
});

function SchedulersComponent(): React.ReactElement {
  const { ws } = Route.useParams();
  return <SchedulerCatalog key={ws} workspaceSlug={ws} />;
}
