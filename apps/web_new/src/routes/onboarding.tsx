// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Onboarding route (AUTH-029). Thin: the funnel guard plus the flow.
// Signed-out visitors bounce to the entry with the return path, finished
// users leave for the landing route, and unfinished signed-in users render
// the single-URL flow. Written from parity/specs/auth.md.

import { ToastHost } from "@pidash/kit";
import { createFileRoute, redirect } from "@tanstack/react-router";
import * as React from "react";

import {
  decidePageGate,
  isOnboardedProfile,
  loadGateInputs,
  loadLandingTarget,
  workspacesQueryOptions,
} from "../core/session/index.js";
import { InviteMembersStep } from "../features/auth/components/InviteMembersStep.js";
import { OnboardingFlow } from "../features/auth/components/OnboardingFlow.js";
import { WorkspaceStep } from "../features/auth/components/WorkspaceStep.js";
import { getToastManager, useSessionExpiredRedirect } from "../shared/shell/index.js";

export const Route = createFileRoute("/onboarding")({
  beforeLoad: async ({ context, location }) => {
    const { me, profile } = await loadGateInputs(context.queryClient, context.apiClient);
    const decision = decidePageGate({
      page: "onboarding-funnel",
      signedIn: me !== null,
      onboarded: isOnboardedProfile(profile),
      hasPassword: me?.is_password_autoset === false,
      currentPath: location.href,
    });
    if (decision.action === "sign-in") {
      throw redirect({ to: "/", search: { next_path: decision.nextPath } });
    }
    if (decision.action === "landing") {
      throw redirect({ href: await loadLandingTarget(context.queryClient, context.apiClient) });
    }
    // NOTE (NEWFRONT-523, size gate): the invitations read stays behind a
    // dynamic import so the invitation query module never joins the
    // initial bundle — this gate only runs on onboarding navigation, after
    // first paint. The gate still resolves before render, as before.
    const { myInvitationsQueryOptions } = await import("../features/auth/index.js");
    // Prefetch the flow's reads; failures fall back to empty inside the flow.
    await Promise.all([
      context.queryClient.ensureQueryData(workspacesQueryOptions(context.apiClient)).catch(() => undefined),
      context.queryClient.ensureQueryData(myInvitationsQueryOptions(context.apiClient)).catch(() => undefined),
    ]);
    return {};
  },
  component: function OnboardingComponent() {
    return <OnboardingScreen />;
  },
});

export function OnboardingScreen(): React.ReactElement {
  useSessionExpiredRedirect();
  return (
    <main>
      <OnboardingFlow steps={{ workspace: WorkspaceStep, invite: InviteMembersStep }} />
      <ToastHost manager={getToastManager()} />
    </main>
  );
}
