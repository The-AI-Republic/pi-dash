// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake screen route (NEWFRONT-689, parity/specs/intake.md INT-001–005,
// 008, 012, 031; the create dialog is NEWFRONT-691, INT-013–017, 033). The
// project layout and tab strip stay around the page; this file adds the two
// search keys that carry the screen state and owns the create dialog: it
// opens from the screen's entry points and, on save, the address moves to
// the new request on the Open tab. The router imports every route file, so
// the screen, the dialog, their reads and the prefetch ride the route's own
// chunk (NEWFRONT-869), started here and awaited in `beforeLoad` beside the
// data.

import { createFileRoute, useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { loadIntakeRouteChunk, resolvedIntakeRouteChunk } from "../../../../../../features/intake/route/lazy.js";
import {
  parseIntakeSearch,
  type IntakeSearch,
  type IntakeSearchWriteOptions,
} from "../../../../../../features/intake/lib/search.js";

// Start the chunk with the route module so the visit does not serialize a
// round trip behind the data reads; `beforeLoad` awaits the same request.
void loadIntakeRouteChunk().catch(() => undefined);

function IntakeRoute(): React.ReactElement {
  const params = Route.useParams();
  const search = Route.useSearch();
  const navigate = useNavigate();
  const { IntakeScreen, IntakeCreateDialog } = resolvedIntakeRouteChunk();
  const [createOpen, setCreateOpen] = React.useState(false);
  const write = React.useCallback(
    (next: IntakeSearch, options?: IntakeSearchWriteOptions) => {
      void navigate({ to: ".", search: next, replace: options?.replace ?? false } as never);
    },
    [navigate]
  );
  const openCreate = React.useCallback(() => {
    setCreateOpen(true);
  }, []);
  // The address is built inline: the writers live in searchWrites.ts on
  // the screen's chunk (NEWFRONT-870), and this eager route module must
  // not pin them back onto the first paint's closure.
  const showCreated = React.useCallback(
    (issueId: string) => {
      write({ currentTab: "open", inboxIssueId: issueId });
    },
    [write]
  );
  return (
    <>
      <IntakeScreen
        workspaceSlug={params.ws}
        projectId={params.projectId}
        search={search}
        onSearchChange={write}
        onCreate={openCreate}
      />
      <IntakeCreateDialog
        workspaceSlug={params.ws}
        projectId={params.projectId}
        open={createOpen}
        onOpenChange={setCreateOpen}
        onCreated={showCreated}
      />
    </>
  );
}

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/intake/")({
  validateSearch: (search: Record<string, unknown>) => parseIntakeSearch(search),
  beforeLoad: async ({ context, search, params }) => {
    const { preloadIntakeRoute } = await loadIntakeRouteChunk();
    await preloadIntakeRoute(context.queryClient, context.apiClient, params, search as Record<string, unknown>);
  },
  component: IntakeRoute,
});
