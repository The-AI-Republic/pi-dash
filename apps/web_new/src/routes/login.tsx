// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Retired sign-in address (AUTH-044). /login forwards to the home route
// as a bare redirect: no email prefill, no return-path handling. The
// canonical sign-in entry is / (see routes/index.tsx). Desktop builds
// render the desktop sign-in screen instead (AUTH-022/AUTH-023).

import { createFileRoute, redirect } from "@tanstack/react-router";
import * as React from "react";

import {
  decidePageGate,
  isOnboardedProfile,
  loadGateInputs,
  loadLandingTarget,
  ONBOARDING_PATH,
} from "../core/session/index.js";
import { DesktopSignInScreen, parseDesktopEntrySearch } from "../features/auth/index.js";
import { platform } from "../core/platform/index.js";

/** Bare forward to the home route; any query params are dropped. */
export function redirectRetiredSignIn(): never {
  throw redirect({ to: "/" });
}

export const Route = createFileRoute("/login")({
  validateSearch: parseDesktopEntrySearch,
  beforeLoad: async ({ context, location, search }) => {
    if (platform.kind !== "desktop") redirectRetiredSignIn();
    const { me, profile } = await loadGateInputs(context.queryClient, context.apiClient);
    const decision = decidePageGate({
      page: "signed-out-only",
      signedIn: me !== null,
      onboarded: isOnboardedProfile(profile),
      hasPassword: me?.is_password_autoset === false,
      currentPath: location.href,
    });
    if (decision.action === "landing") {
      throw redirect({ href: await loadLandingTarget(context.queryClient, context.apiClient, search.nextPath) });
    }
    if (decision.action === "onboarding") {
      throw redirect({ href: ONBOARDING_PATH });
    }
    return {};
  },
  component: DesktopLoginScreen,
});

/**
 * Desktop sign-in screen. The web build never renders this: its
 * beforeLoad already redirected to the entry. Module-local (not
 * exported): only this form lets the router split the route component
 * out of the entry.
 */
function DesktopLoginScreen(): React.ReactElement | null {
  const search = Route.useSearch();
  if (platform.kind !== "desktop") return null;
  return <DesktopSignInScreen error={search.error} errorDescription={search.errorDescription} />;
}
