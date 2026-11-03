// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Forgot-password route. Thin: search schema, signed-out-only guard,
// card. Signed-in visitors leave for the entry route (the guards slice
// owns the full funnel; this keeps the signed-out-only rule per the spec).
// Written from parity/specs/auth.md AUTH-012.

import { ToastHost } from "@pidash/kit";
import { createFileRoute, redirect } from "@tanstack/react-router";
import * as React from "react";

import { meQueryOptions } from "../../core/session/queries.js";
import { asString } from "../../features/auth/entrySearch.js";
import { ForgotPasswordCard } from "../../features/auth/components/ForgotPasswordCard.js";
import { getToastManager } from "../../shared/shell/index.js";

export interface ForgotPasswordSearch {
  email?: string | undefined;
}

/** Search schema: invalid params fall back to absent instead of throwing. */
export function parseForgotPasswordSearch(search: Record<string, unknown>): ForgotPasswordSearch {
  const email = asString(search["email"]);
  return { ...(email === undefined ? {} : { email }) };
}

export const Route = createFileRoute("/accounts/forgot-password")({
  validateSearch: parseForgotPasswordSearch,
  beforeLoad: async ({ context }) => {
    const me = await context.queryClient.ensureQueryData(meQueryOptions(context.apiClient)).catch(() => null);
    if (me) {
      throw redirect({ to: "/" });
    }
    return {};
  },
  component: function ForgotPasswordComponent() {
    const search = Route.useSearch();
    return <ForgotPasswordScreen email={search.email} />;
  },
});

export function ForgotPasswordScreen({ email }: { email?: string | undefined }): React.ReactElement {
  return (
    <main className="flex min-h-screen items-center justify-center bg-(--bg) p-(--space-8)">
      <ForgotPasswordCard initialEmail={email ?? ""} />
      <ToastHost manager={getToastManager()} />
    </main>
  );
}
