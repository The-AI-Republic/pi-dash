// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Set-password route. Thin: guard plus card. Signed-out visitors bounce
// to the entry with the return path; signed-in users who already hold a
// secret and are onboarded leave for the entry (which routes them on);
// everyone else sets their first secret here. The guards slice owns the
// guard framework and the post-auth landing; this keeps the set-password
// rule per the spec until then. Written from parity/specs/auth.md
// AUTH-014.

import { ToastHost } from "@pidash/kit";
import { createFileRoute, redirect } from "@tanstack/react-router";
import * as React from "react";

import { isOnboardedProfile } from "../../core/session/guards.js";
import { meQueryOptions, profileQueryOptions } from "../../core/session/queries.js";
import { SetPasswordCard } from "../../features/auth/components/SetPasswordCard.js";
import { getToastManager, useSessionExpiredRedirect } from "../../shared/shell/index.js";

export type SetPasswordGateDecision = "render" | "away";

/**
 * The set-password guard rule (AUTH-014): passwordless accounts render;
 * users who already hold a secret and are onboarded are sent away. An
 * unreadable profile fails open to the form — the set POST rechecks.
 */
export function decideSetPasswordGate(args: {
  passwordAutoset: boolean;
  onboarded: boolean | null;
}): SetPasswordGateDecision {
  if (args.passwordAutoset) return "render";
  if (args.onboarded === true) return "away";
  return "render";
}

export const Route = createFileRoute("/accounts/set-password")({
  beforeLoad: async ({ context, location }) => {
    const me = await context.queryClient.ensureQueryData(meQueryOptions(context.apiClient)).catch(() => null);
    if (!me) {
      throw redirect({ to: "/", search: { next_path: location.href } });
    }
    const profile = await context.queryClient.ensureQueryData(profileQueryOptions(context.apiClient)).catch(() => null);
    if (
      decideSetPasswordGate({
        passwordAutoset: me.is_password_autoset,
        onboarded: profile === null ? null : isOnboardedProfile(profile),
      }) === "away"
    ) {
      throw redirect({ to: "/" });
    }
    return {};
  },
  component: function SetPasswordComponent() {
    return <SetPasswordScreen />;
  },
});

export function SetPasswordScreen(): React.ReactElement {
  useSessionExpiredRedirect();
  return (
    <main className="flex min-h-screen items-center justify-center bg-(--bg) p-(--space-8)">
      <SetPasswordCard />
      <ToastHost manager={getToastManager()} />
    </main>
  );
}
