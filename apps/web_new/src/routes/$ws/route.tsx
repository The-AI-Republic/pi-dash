// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace route. Thin: membership gate only, then the outlet. Known
// workspace pages additionally pass the sign-in gate in the `_gated`
// layout; the workspace catch-all (`/$ws/$`) is not a child of that
// layout, so unknown addresses render the illustrated not-found page with
// or without a session. Signed-in visitors on an unknown slug see the
// workspace access notice below.

import { createFileRoute, notFound, Outlet } from "@tanstack/react-router";

import { decideWorkspaceRouteGate, meQueryOptions, workspacesQueryOptions } from "../../core/session/queries.js";

export const Route = createFileRoute("/$ws")({
  beforeLoad: async ({ context, params }) => {
    const me = await context.queryClient.ensureQueryData(meQueryOptions(context.apiClient)).catch(() => null);
    const workspaces = me
      ? await context.queryClient.ensureQueryData(workspacesQueryOptions(context.apiClient)).catch(() => undefined)
      : undefined;
    if (decideWorkspaceRouteGate({ signedIn: me !== null, workspaces, slugOrId: params.ws }) === "not-found") {
      throw notFound();
    }
    return {};
  },
  notFoundComponent: function WorkspaceAccessNotice() {
    return (
      <main className="flex min-h-screen flex-col items-center justify-center gap-(--space-2) bg-(--bg) p-(--space-8)">
        <h1 className="text-h2 text-(--text)">Workspace not found</h1>
        <p className="text-body text-(--text-muted)">
          You may not belong to it, or the address is wrong. Check your invitations or the workspace list.
        </p>
      </main>
    );
  },
  component: function WorkspaceRouteComponent() {
    return <Outlet />;
  },
});
