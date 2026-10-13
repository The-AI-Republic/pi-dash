// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Entrypoint route. Signed-out visitors land here (the workspace gate
// sends them over) and get the sign-in card in place; signed-in users
// continue to onboarding or their AUTH-016 landing (last or fallback
// workspace, else workspace creation). The return path also accepts the
// `next_path` spelling guards emit and old links carry.

import { createFileRoute, redirect } from "@tanstack/react-router";
import * as React from "react";

import {
  asString,
  decidePageGate,
  loadGateInputs,
  loadLandingTarget,
  ONBOARDING_PATH,
  parseReturnPath,
} from "../core/session/index.js";
import { SignInCard } from "../features/auth/index.js";

export interface EntrySearch {
  next?: string | undefined;
  /**
   * Canonical return-path spelling (spec AUTH-015/016); `next` stays as an
   * alias. Accepted on input and normalized to `next`/`unsafeNext` by the
   * parser; construct bounce searches with this field so entry URLs keep
   * the parity spelling.
   */
  next_path?: string | undefined;
  /** Raw return path for the auth POSTs; never navigated to client-side. */
  unsafeNext?: string | undefined;
  email?: string | undefined;
  error_code?: string | undefined;
  invitation_id?: string | undefined;
  slug?: string | undefined;
}

/** Search schema: invalid params fall back to absent instead of throwing. */
export function parseEntrySearch(search: Record<string, unknown>): EntrySearch {
  const { next, unsafeNext } = parseReturnPath(search);
  const email = asString(search["email"]);
  const errorCode = asString(search["error_code"]);
  const invitationId = asString(search["invitation_id"]);
  const slug = asString(search["slug"]);
  return {
    ...(next === undefined ? {} : { next }),
    ...(unsafeNext === undefined ? {} : { unsafeNext }),
    ...(email === undefined ? {} : { email }),
    ...(errorCode === undefined ? {} : { error_code: errorCode }),
    ...(invitationId === undefined ? {} : { invitation_id: invitationId }),
    ...(slug === undefined ? {} : { slug }),
  };
}

/** Effective return path: the canonical param wins over the alias. */
export function returnPath(search: { next?: string | undefined; next_path?: string | undefined }): string | undefined {
  return search.next_path ?? search.next;
}

export const Route = createFileRoute("/")({
  validateSearch: parseEntrySearch,
  beforeLoad: async ({ context, search, location }) => {
    const { me, profile } = await loadGateInputs(context.queryClient, context.apiClient);
    const decision = decidePageGate({
      page: "signed-out-only",
      signedIn: me !== null,
      onboarded: profile?.is_onboarded ?? false,
      hasPassword: me?.is_password_autoset === false,
      currentPath: location.href,
    });
    if (decision.action === "landing") {
      throw redirect({ href: await loadLandingTarget(context.queryClient, context.apiClient, search.next) });
    }
    if (decision.action === "onboarding") {
      throw redirect({ href: ONBOARDING_PATH });
    }
    return {};
  },
  component: function HomeComponent() {
    const search = Route.useSearch();
    return (
      <EntryScreen
        next={search.next}
        unsafeNext={search.unsafeNext}
        email={search.email}
        errorCode={search.error_code}
        invitationId={search.invitation_id}
        invitationSlug={search.slug}
      />
    );
  },
});

export function EntryScreen({
  next,
  unsafeNext,
  email,
  errorCode,
  invitationId,
  invitationSlug,
}: {
  next?: string | undefined;
  unsafeNext?: string | undefined;
  email?: string | undefined;
  errorCode?: string | undefined;
  invitationId?: string | undefined;
  invitationSlug?: string | undefined;
}): React.ReactElement {
  return (
    <main className="flex min-h-screen items-center justify-center bg-(--bg) p-(--space-8)">
      <SignInCard
        next={next}
        unsafeNext={unsafeNext}
        initialEmail={email ?? ""}
        initialErrorCode={errorCode ?? null}
        invitationId={invitationId}
        invitationSlug={invitationSlug}
      />
    </main>
  );
}
