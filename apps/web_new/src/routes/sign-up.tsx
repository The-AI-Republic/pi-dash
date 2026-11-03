// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sign-up route. Thin: search schema, session redirect, lazy card.
// The canonical entry (/sign-up) prefills the card from the email param
// and carries the return path through the steps as next_path (AUTH-011);
// signed-in visitors continue to onboarding or their AUTH-016 landing.

import { createFileRoute, redirect } from "@tanstack/react-router";
import type { QueryClient } from "@tanstack/react-query";
import type { ApiClient } from "@pidash/api-client";
import * as React from "react";

import {
  decidePageGate,
  isOnboardedProfile,
  loadGateInputs,
  loadLandingTarget,
  ONBOARDING_PATH,
} from "../core/session/index.js";
import { asString, safeNextPath } from "../features/auth/entrySearch.js";
import { SignUpCard } from "../features/auth/components/SignUpCard.js";

export interface SignUpSearch {
  next_path?: string | undefined;
  email?: string | undefined;
  error_code?: string | undefined;
  invitation_id?: string | undefined;
  slug?: string | undefined;
}

/** Search schema: invalid params fall back to absent instead of throwing. */
export function parseSignUpSearch(search: Record<string, unknown>): SignUpSearch {
  const nextPath = safeNextPath(search["next_path"]);
  const email = asString(search["email"]);
  const errorCode = asString(search["error_code"]);
  const invitationId = asString(search["invitation_id"]);
  const slug = asString(search["slug"]);
  return {
    ...(nextPath === undefined ? {} : { next_path: nextPath }),
    ...(email === undefined ? {} : { email }),
    ...(errorCode === undefined ? {} : { error_code: errorCode }),
    ...(invitationId === undefined ? {} : { invitation_id: invitationId }),
    ...(slug === undefined ? {} : { slug }),
  };
}

export interface SignUpGuardContext {
  queryClient: QueryClient;
  apiClient: ApiClient;
}

/** Signed-out-only gate: visitors see the card, members move on. */
export async function redirectSignedInUser({
  context,
  search,
}: {
  context: SignUpGuardContext;
  search: SignUpSearch;
}): Promise<Record<string, never>> {
  const { me, profile } = await loadGateInputs(context.queryClient, context.apiClient);
  const decision = decidePageGate({
    page: "signed-out-only",
    signedIn: me !== null,
    onboarded: isOnboardedProfile(profile),
    hasPassword: me?.is_password_autoset === false,
    // A signed-out-only page never bounces to sign-in, so the return
    // path that decision would carry is unreachable; the value only
    // fills the decider's input.
    currentPath: "/sign-up",
  });
  if (decision.action === "landing") {
    throw redirect({ href: await loadLandingTarget(context.queryClient, context.apiClient, search.next_path) });
  }
  if (decision.action === "onboarding") {
    throw redirect({ href: ONBOARDING_PATH });
  }
  return {};
}

export const Route = createFileRoute("/sign-up")({
  validateSearch: parseSignUpSearch,
  beforeLoad: redirectSignedInUser,
  component: function SignUpComponent() {
    const search = Route.useSearch();
    return <SignUpScreen search={search} />;
  },
});

export function SignUpScreen({ search }: { search: SignUpSearch }): React.ReactElement {
  return (
    <main className="flex min-h-screen items-center justify-center bg-(--bg) p-(--space-8)">
      <SignUpCard
        next={search.next_path}
        initialEmail={search.email ?? ""}
        initialErrorCode={search.error_code ?? null}
        invitationId={search.invitation_id}
        invitationSlug={search.slug}
      />
    </main>
  );
}
