// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sign-in route. Thin: search schema, session redirect, lazy card.
// Signed-in visitors continue to their return path or first workspace;
// everyone else gets the card with the server error (if any) preloaded.

import { createFileRoute, redirect } from "@tanstack/react-router";

import { meQueryOptions, workspacesQueryOptions } from "../core/session/queries.js";
import { SignInCard } from "../features/auth/index.js";

interface SignInSearch {
  next?: string | undefined;
  email?: string | undefined;
  error_code?: string | undefined;
}

function asString(value: unknown): string | undefined {
  return typeof value === "string" && value.length > 0 ? value : undefined;
}

/** Internal return paths only; anything else falls back to post-auth landing. */
export function safeNextPath(value: unknown): string | undefined {
  const path = asString(value);
  if (!path || !path.startsWith("/") || path.startsWith("//")) return undefined;
  return path;
}

export const Route = createFileRoute("/sign-in")({
  validateSearch: (search: Record<string, unknown>): SignInSearch => ({
    ...(safeNextPath(search["next"]) === undefined ? {} : { next: safeNextPath(search["next"]) }),
    ...(asString(search["email"]) === undefined ? {} : { email: asString(search["email"]) }),
    ...(asString(search["error_code"]) === undefined ? {} : { error_code: asString(search["error_code"]) }),
  }),
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
    return (
      <main className="flex min-h-screen items-center justify-center bg-(--bg) p-(--space-8)">
        <SignInCard next={search.next} initialEmail={search.email ?? ""} initialErrorCode={search.error_code ?? null} />
      </main>
    );
  },
});
