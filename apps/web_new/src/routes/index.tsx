// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Entrypoint route. Signed-out visitors go to sign-in; signed-in users
// continue into their first workspace. Users without a workspace see a
// notice until invitations and workspace creation land (NEWFRONT-69).

import { createFileRoute, redirect } from "@tanstack/react-router";

import { meQueryOptions, workspacesQueryOptions } from "../core/session/queries.js";

export const Route = createFileRoute("/")({
  beforeLoad: async ({ context }) => {
    const me = await context.queryClient.ensureQueryData(meQueryOptions(context.apiClient)).catch(() => null);
    if (!me) {
      throw redirect({ to: "/sign-in" });
    }
    const workspaces = await context.queryClient
      .ensureQueryData(workspacesQueryOptions(context.apiClient))
      .catch(() => []);
    const first = workspaces[0];
    if (first) {
      throw redirect({ to: "/$ws", params: { ws: first.slug } });
    }
    return {};
  },
  component: function HomeComponent() {
    return (
      <main className="flex min-h-screen flex-col items-center justify-center gap-(--space-2) bg-(--bg) p-(--space-8)">
        <h1 className="text-h2 text-(--text)">Welcome to Pi Dash</h1>
        <p className="text-body text-(--text-muted)">You are signed in but do not belong to a workspace yet.</p>
      </main>
    );
  },
});
