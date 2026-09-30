// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
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
