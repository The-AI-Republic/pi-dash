// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sign-in route. Thin: search schema, session redirect, lazy card.
// Signed-in visitors continue to their return path or first workspace;
// everyone else gets the card with the server error (if any) preloaded.

import { createFileRoute, redirect } from "@tanstack/react-router";
import * as React from "react";

import { meQueryOptions, workspacesQueryOptions } from "../core/session/queries.js";
import { SignInCard } from "../features/auth/index.js";

export interface SignInSearch {
  next?: string | undefined;
  email?: string | undefined;
  error_code?: string | undefined;
  invitation_id?: string | undefined;
  slug?: string | undefined;
}

export function asString(value: unknown): string | undefined {
  return typeof value === "string" && value.length > 0 ? value : undefined;
}

/** Internal return paths only; anything else falls back to post-auth landing. */
export function safeNextPath(value: unknown): string | undefined {
  const path = asString(value);
  if (!path || !path.startsWith("/") || path.startsWith("//")) return undefined;
  return path;
}

/** Search schema: invalid params fall back to absent instead of throwing. */
export function parseSignInSearch(search: Record<string, unknown>): SignInSearch {
  const next = safeNextPath(search["next"]);
  const email = asString(search["email"]);
  const errorCode = asString(search["error_code"]);
  const invitationId = asString(search["invitation_id"]);
  const slug = asString(search["slug"]);
  return {
    ...(next === undefined ? {} : { next }),
    ...(email === undefined ? {} : { email }),
    ...(errorCode === undefined ? {} : { error_code: errorCode }),
    ...(invitationId === undefined ? {} : { invitation_id: invitationId }),
    ...(slug === undefined ? {} : { slug }),
  };
}

export const Route = createFileRoute("/sign-in")({
  validateSearch: parseSignInSearch,
  beforeLoad: async ({ context, search }) => {
    const me = await context.queryClient.ensureQueryData(meQueryOptions(context.apiClient)).catch(() => null);
    if (!me) return {};
    if (search.next) {
      throw redirect({ href: search.next });
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
  component: function SignInComponent() {
    const search = Route.useSearch();
    return <SignInScreen search={search} />;
  },
});

export function SignInScreen({ search }: { search: SignInSearch }): React.ReactElement {
  return (
    <main className="flex min-h-screen items-center justify-center bg-(--bg) p-(--space-8)">
      <SignInCard
        next={search.next}
        initialEmail={search.email ?? ""}
        initialErrorCode={search.error_code ?? null}
        invitationId={search.invitation_id}
        invitationSlug={search.slug}
      />
    </main>
  );
}
