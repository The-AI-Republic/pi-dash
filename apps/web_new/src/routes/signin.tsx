// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Retired sign-in address (AUTH-044). Web builds forward bare to the home
// route; desktop builds render the same bare sign-in screen as the other
// entry addresses (AUTH-022/AUTH-023, DESK-011/DESK-012).

import { createFileRoute, redirect } from "@tanstack/react-router";
import * as React from "react";

import {
  decidePageGate,
  isOnboardedProfile,
  loadGateInputs,
  loadLandingTarget,
  ONBOARDING_PATH,
} from "../core/session/index.js";
import { DesktopSignInScreen } from "../features/auth/components/DesktopSignInScreen.js";
import { parseDesktopEntrySearch } from "../features/auth/desktop.js";
import { platform } from "../core/platform/index.js";

/** Bare forward to the home route; any query params are dropped. */
export function redirectRetiredSignIn(): never {
  throw redirect({ to: "/" });
}

export const Route = createFileRoute("/signin")({
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
  component: DesktopSigninScreen,
});

function DesktopSigninScreen(): React.ReactElement | null {
  const search = Route.useSearch();
  if (platform.kind !== "desktop") return null;
  return <DesktopSignInScreen error={search.error} errorDescription={search.errorDescription} />;
}
