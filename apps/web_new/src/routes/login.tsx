// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sign-in address (AUTH-044, CLOUD-002). The cloud edition fills
// `slots.cloudLoginCard` and serves its organizational-account card at
// /login in place (CLOUD-011-014, NEWFRONT-652). Every other build treats
// /login as a retired address and forwards to the root, which owns the
// community entry card. Desktop builds render the desktop sign-in screen
// instead (AUTH-022/AUTH-023).

import { createFileRoute, redirect } from "@tanstack/react-router";
import * as React from "react";

import { edition, type Edition } from "../core/edition/index.js";
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

/**
 * Whether this build serves a cloud sign-in page at /login. The edition
 * owns the address through the card slot: a module that fills it gets the
 * card in place, while the OSS module fills nothing and /login keeps
 * forwarding to the root.
 */
export function rendersLoginInPlace(candidate: Edition): boolean {
  return platform.kind === "web" && candidate.slots?.cloudLoginCard !== undefined;
}

export const Route = createFileRoute("/login")({
  // The desktop landing reads the parsed failure fields; the cloud card
  // reads `next_path` and `error` from the address itself, so the visitor's
  // URL stays exactly as they arrived.
  validateSearch: parseDesktopEntrySearch,
  beforeLoad: async ({ context, location, search }) => {
    if (platform.kind !== "desktop" && !rendersLoginInPlace(edition)) redirectRetiredSignIn();
    const parsed = parseDesktopEntrySearch(search);
    const { me, profile } = await loadGateInputs(context.queryClient, context.apiClient);
    const decision = decidePageGate({
      page: "signed-out-only",
      signedIn: me !== null,
      onboarded: isOnboardedProfile(profile),
      hasPassword: me?.is_password_autoset === false,
      currentPath: location.href,
    });
    if (decision.action === "landing") {
      throw redirect({ href: await loadLandingTarget(context.queryClient, context.apiClient, parsed.nextPath) });
    }
    if (decision.action === "onboarding") {
      throw redirect({ href: ONBOARDING_PATH });
    }
    return {};
  },
  component: LoginScreen,
});

/**
 * Login screen. Desktop builds show the desktop landing; a cloud web build
 * shows the edition's sign-in card. Every other web build never renders
 * this: its beforeLoad forwarded already.
 */
function LoginScreen(): React.ReactElement | null {
  if (platform.kind === "desktop") {
    const search = Route.useSearch();
    return <DesktopSignInScreen error={search.error} errorDescription={search.errorDescription} />;
  }
  const CloudLoginCard = edition.slots?.cloudLoginCard;
  if (CloudLoginCard === undefined) return null;
  return <CloudLoginCard />;
}
