// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Reset-password route. Thin: search schema, signed-out-only guard, card.
// The emailed link carries uidb64, token, and the address; the card shows
// the address locked and replaces itself with the banner for dead links.
// Written from parity/specs/auth.md AUTH-013.

import { createFileRoute, redirect } from "@tanstack/react-router";
import * as React from "react";

import { meQueryOptions } from "../../core/session/queries.js";
import { asString } from "../../features/auth/entrySearch.js";
import { ResetPasswordCard } from "../../features/auth/components/ResetPasswordCard.js";

export interface ResetPasswordSearch {
  uidb64?: string | undefined;
  token?: string | undefined;
  email?: string | undefined;
}

/** Search schema: invalid params fall back to absent instead of throwing. */
export function parseResetPasswordSearch(search: Record<string, unknown>): ResetPasswordSearch {
  const uidb64 = asString(search["uidb64"]);
  const token = asString(search["token"]);
  const email = asString(search["email"]);
  return {
    ...(uidb64 === undefined ? {} : { uidb64 }),
    ...(token === undefined ? {} : { token }),
    ...(email === undefined ? {} : { email }),
  };
}

export const Route = createFileRoute("/accounts/reset-password")({
  validateSearch: parseResetPasswordSearch,
  beforeLoad: async ({ context }) => {
    const me = await context.queryClient.ensureQueryData(meQueryOptions(context.apiClient)).catch(() => null);
    if (me) {
      throw redirect({ to: "/" });
    }
    return {};
  },
  component: function ResetPasswordComponent() {
    const search = Route.useSearch();
    return <ResetPasswordScreen uidb64={search.uidb64} token={search.token} email={search.email} />;
  },
});

export function ResetPasswordScreen({
  uidb64,
  token,
  email,
}: {
  uidb64?: string | undefined;
  token?: string | undefined;
  email?: string | undefined;
}): React.ReactElement {
  return (
    <main className="flex min-h-screen items-center justify-center bg-(--bg) p-(--space-8)">
      <ResetPasswordCard uid={uidb64 ?? ""} token={token ?? ""} email={email} />
    </main>
  );
}
