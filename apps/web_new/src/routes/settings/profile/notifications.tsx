// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Email preferences route (parity/specs/notifications.md §10,
// NTF-024/025/031). Signed-in-only: visitors bounce to the entry with
// the return path and unfinished members to onboarding. The page renders
// the notifications feature's preferences content directly; the profile
// settings shell (PROF-009/010/026) slots this same component into its
// tab registry when it lands. Written from the notifications area spec.

import { ToastHost } from "@pidash/kit";
import { createFileRoute, redirect } from "@tanstack/react-router";
import * as React from "react";

import { decidePageGate, isOnboardedProfile, loadGateInputs, ONBOARDING_PATH } from "../../../core/session/index.js";
import { EmailPreferencesPage } from "../../../features/notifications/index.js";
import { getToastManager, useSessionExpiredRedirect } from "../../../shared/shell/index.js";

export const Route = createFileRoute("/settings/profile/notifications")({
  beforeLoad: async ({ context, location }) => {
    const { me, profile } = await loadGateInputs(context.queryClient, context.apiClient);
    const decision = decidePageGate({
      page: "signed-in-only",
      signedIn: me !== null,
      onboarded: isOnboardedProfile(profile),
      hasPassword: me?.is_password_autoset === false,
      currentPath: location.href,
    });
    if (decision.action === "sign-in") {
      throw redirect({ to: "/", search: { next_path: decision.nextPath } });
    }
    if (decision.action === "onboarding") {
      throw redirect({ href: ONBOARDING_PATH });
    }
    return {};
  },
  component: function EmailPreferencesRouteComponent() {
    return <EmailPreferencesScreen />;
  },
});

export function EmailPreferencesScreen(): React.ReactElement {
  useSessionExpiredRedirect();
  return (
    <main className="min-h-screen bg-(--bg)">
      <div className="mx-auto w-full max-w-2xl px-(--space-6) py-(--space-8)">
        <EmailPreferencesPage />
      </div>
      <ToastHost manager={getToastManager()} />
    </main>
  );
}
