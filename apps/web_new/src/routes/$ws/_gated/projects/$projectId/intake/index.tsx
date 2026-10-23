// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake screen route (NEWFRONT-689, parity/specs/intake.md INT-001–005,
// 008, 012, 031). The project layout and tab strip stay around the page;
// this file only adds the two search keys that carry the screen state. The
// router imports every route file, so the screen, its reads and their
// prefetch ride the route's own chunk (NEWFRONT-869), started here and
// awaited in `beforeLoad` beside the data.

import { createFileRoute, useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { loadIntakeRouteChunk, resolvedIntakeRouteChunk } from "../../../../../../features/intake/route/lazy.js";
import { parseIntakeSearch, type IntakeSearch } from "../../../../../../features/intake/lib/search.js";

// Start the chunk with the route module so the visit does not serialize a
// round trip behind the data reads; `beforeLoad` awaits the same request.
void loadIntakeRouteChunk().catch(() => undefined);

function IntakeRoute(): React.ReactElement {
  const params = Route.useParams();
  const search = Route.useSearch();
  const navigate = useNavigate();
  const { IntakeScreen } = resolvedIntakeRouteChunk();
  const write = React.useCallback(
    (next: IntakeSearch) => {
      void navigate({ to: ".", search: next, replace: true } as never);
    },
    [navigate]
  );
  return <IntakeScreen workspaceSlug={params.ws} projectId={params.projectId} search={search} onSearchChange={write} />;
}

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/intake/")({
  validateSearch: (search: Record<string, unknown>) => parseIntakeSearch(search),
  beforeLoad: async ({ context, search, params }) => {
    const { preloadIntakeRoute } = await loadIntakeRouteChunk();
    await preloadIntakeRoute(context.queryClient, context.apiClient, params, search as Record<string, unknown>);
  },
  component: IntakeRoute,
});
