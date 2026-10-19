// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Personal access token leaf route (parity/specs/workspace-settings.md §25,
// §41 seam). Signed-in-only: visitors bounce to the entry with the return
// path and unfinished members to onboarding. The profile settings shell
// mounts this same leaf later; the page renders without that layout.

import { ToastHost } from "@pidash/kit";
import { createFileRoute, redirect } from "@tanstack/react-router";
import * as React from "react";

import { decidePageGate, isOnboardedProfile, loadGateInputs, ONBOARDING_PATH } from "../../../core/session/index.js";
import { PersonalAccessTokensPage } from "../../../features/workspace-settings/tokens/index.js";
import { getToastManager, useSessionExpiredRedirect } from "../../../shared/shell/index.js";

export const Route = createFileRoute("/settings/profile/api-tokens")({
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
  component: PersonalAccessTokensRouteComponent,
});

export function PersonalAccessTokensRouteComponent(): React.ReactElement {
  useSessionExpiredRedirect();
  return (
    <main className="min-h-screen bg-(--bg)">
      <div className="mx-auto w-full max-w-3xl px-(--space-6) py-(--space-8)">
        <PersonalAccessTokensPage />
      </div>
      <ToastHost manager={getToastManager()} />
    </main>
  );
}
