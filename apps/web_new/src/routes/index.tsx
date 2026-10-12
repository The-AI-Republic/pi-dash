// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Entrypoint route. Signed-out visitors land here (the workspace gate
// sends them over) and get the sign-in card in place; signed-in users
// continue into their first workspace. Users without a workspace see a
// notice until invitations and workspace creation land (NEWFRONT-69).

import { createFileRoute, redirect } from "@tanstack/react-router";
import * as React from "react";

import { meQueryOptions, workspacesQueryOptions } from "../core/session/queries.js";
import { SignInCard } from "../features/auth/index.js";
import { safeNextPath } from "./sign-in.js";

interface EntrySearch {
  next?: string | undefined;
}

export const Route = createFileRoute("/")({
  validateSearch: (search: Record<string, unknown>): EntrySearch => {
    const next = safeNextPath(search["next"]);
    return next === undefined ? {} : { next };
  },
  beforeLoad: async ({ context }) => {
    const me = await context.queryClient.ensureQueryData(meQueryOptions(context.apiClient)).catch(() => null);
    if (!me) return {};
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
    const search = Route.useSearch();
    const context = Route.useRouteContext();
    return (
      <EntryScreen
        signedIn={context.queryClient.getQueryData(meQueryOptions(context.apiClient).queryKey) != null}
        next={search.next}
      />
    );
  },
});

export function EntryScreen({ signedIn, next }: { signedIn: boolean; next?: string | undefined }): React.ReactElement {
  if (!signedIn) {
    return (
      <main className="flex min-h-screen items-center justify-center bg-(--bg) p-(--space-8)">
        <SignInCard next={next} initialEmail="" initialErrorCode={null} />
      </main>
    );
  }
  return (
    <main className="flex min-h-screen flex-col items-center justify-center gap-(--space-2) bg-(--bg) p-(--space-8)">
      <h1 className="text-h2 text-(--text)">Welcome to Pi Dash</h1>
      <p className="text-body text-(--text-muted)">You are signed in but do not belong to a workspace yet.</p>
    </main>
  );
}
