// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Assistant layout route (parity/specs/agents.md §9/§11). Prefetches the
// conversation list the rail renders and mounts the rail beside the
// routed landing or conversation. The list read is tolerated: guests are
// refused server-side, and the rail has to render that refusal as an
// empty, placeholder-free sidebar (AGT-041).

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { assistantThreadsQueryOptions, AssistantLayout } from "../../../../features/assistant/index.js";

export const Route = createFileRoute("/$ws/_gated/assistant")({
  loader: ({ context, params }) =>
    context.queryClient
      .ensureQueryData(assistantThreadsQueryOptions(context.apiClient, params.ws))
      .catch(() => undefined),
  component: AssistantLayoutComponent,
});

function AssistantLayoutComponent(): React.ReactElement {
  const { ws } = Route.useParams();
  return <AssistantLayout key={ws} workspaceSlug={ws} />;
}
