// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake screen (NEWFRONT-689, parity/specs/intake.md INT-001–005, 008,
// 012, 031). The route keeps the project layout and tab strip; it adds
// the two search keys that carry the screen state and prefetches the
// first page the screen will ask for. The gate itself is the screen's
// job (the project row is its single read), so the prefetch only warms
// the list when the screen is going to render.

import { createFileRoute, useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { projectQueryOptions } from "../../../../../../features/auth/index.js";
import { prefetchIntakeList } from "../../../../../../features/intake/api/queries.js";
import { IntakeScreen } from "../../../../../../features/intake/components/IntakeScreen.js";
import { parseIntakeSearch, type IntakeSearch } from "../../../../../../features/intake/lib/search.js";

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/intake/")({
  validateSearch: (search: Record<string, unknown>) => parseIntakeSearch(search),
  beforeLoad: async ({ context, search, params }) => {
    const parsed = parseIntakeSearch(search);
    const project = await context.queryClient
      .ensureQueryData(projectQueryOptions(context.apiClient, params.ws, params.projectId))
      .catch(() => undefined);
    if (project === undefined || !project.inbox_view) return;
    await prefetchIntakeList(context.queryClient, context.apiClient, params.ws, params.projectId, {
      tab: parsed.currentTab,
    });
  },
  component: IntakeRoute,
});

function IntakeRoute(): React.ReactElement {
  const params = Route.useParams();
  const search = Route.useSearch();
  const navigate = useNavigate();
  const write = React.useCallback(
    (next: IntakeSearch) => {
      void navigate({ to: ".", search: next, replace: true } as never);
    },
    [navigate]
  );
  return <IntakeScreen workspaceSlug={params.ws} projectId={params.projectId} search={search} onSearchChange={write} />;
}
