// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Invitations route (AUTH-026). Thin: session gate, inbox screen.
// Signed-in-only: visitors bounce to the entry route with a return path,
// unfinished members funnel to onboarding, and only onboarded members
// render the inbox.

import { createFileRoute, redirect } from "@tanstack/react-router";

import { decidePageGate, loadGateInputs, ONBOARDING_PATH } from "../core/session/index.js";
import { InvitationInbox } from "../features/auth/components/InvitationInbox.js";

export const Route = createFileRoute("/invitations")({
  beforeLoad: async ({ context, location }) => {
    const { me, profile } = await loadGateInputs(context.queryClient, context.apiClient);
    const decision = decidePageGate({
      page: "signed-in-only",
      signedIn: me !== null,
      onboarded: profile?.is_onboarded ?? false,
      hasPassword: me?.is_password_autoset === false,
      currentPath: location.href,
    });
    if (decision.action === "sign-in") {
      // Land on the entry route, which renders the sign-in card in place:
      // the return path survives so sign-in continues at the inbox.
      throw redirect({ to: "/", search: { next_path: decision.nextPath } });
    }
    if (decision.action === "onboarding") {
      throw redirect({ href: ONBOARDING_PATH });
    }
    return {};
  },
  component: InvitationInbox,
});
