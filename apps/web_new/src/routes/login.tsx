// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sign-in address (AUTH-044, CLOUD-002, CLOUD-011-014). A web build serves
// /login in place when the edition owns the address: either it declares
// /login as a top-level route (NEWFRONT-645) or it fills
// `slots.cloudLoginCard` (NEWFRONT-652). The screen shows the edition's
// card when the slot is filled, else the community entry card with the
// same return-path handling as the root. Without either, /login is a
// retired address and forwards to the root, which owns the entry card.
// Desktop builds keep their own landing screen (AUTH-022/AUTH-023) on
// every edition.
import { createFileRoute, redirect } from "@tanstack/react-router";
import * as React from "react";

import { edition, isEditionPublicRouteEnabled, type Edition } from "../core/edition/index.js";
import {
  decidePageGate,
  isOnboardedProfile,
  loadGateInputs,
  loadLandingTarget,
  ONBOARDING_PATH,
} from "../core/session/index.js";
import { DesktopSignInScreen } from "../features/auth/components/DesktopSignInScreen.js";
import { EntryScreen } from "../features/auth/components/EntryScreen.js";
import { parseLoginSearch } from "../features/auth/components/LoginSearch.js";
import { platform } from "../core/platform/index.js";

/** Bare forward to the home route; any query params are dropped. */
export function redirectRetiredSignIn(): never {
  throw redirect({ to: "/" });
}

/**
 * True when this edition serves the sign-in page at /login instead of
 * forwarding it (CLOUD-002, CLOUD-011). Either ownership signal counts:
 * the top-level route declaration, so a cloud build is correct as soon as
 * the module declares the route, or the filled card slot. The card the
 * screen renders is a separate concern.
 */
export function rendersLoginInPlace(candidate: Edition): boolean {
  return (
    platform.kind === "web" &&
    (candidate.slots?.cloudLoginCard !== undefined || isEditionPublicRouteEnabled(candidate, "/login"))
  );
}

export const Route = createFileRoute("/login")({
  // Keep the address exactly as the visitor arrived: the escape hatch
  // carries `next_path` and the oracle asserts the URL it lands on. The
  // guard and the screen parse what they need; a validating schema would
  // rewrite the address with derived params.
  validateSearch: (search: Record<string, unknown>) => search,
  beforeLoad: async ({ context, location, search }) => {
    if (platform.kind !== "desktop" && !rendersLoginInPlace(edition)) redirectRetiredSignIn();
    const parsed = parseLoginSearch(search);
    const { me, profile } = await loadGateInputs(context.queryClient, context.apiClient);
    const decision = decidePageGate({
      page: "signed-out-only",
      signedIn: me !== null,
      onboarded: isOnboardedProfile(profile),
      hasPassword: me?.is_password_autoset === false,
      currentPath: location.href,
    });
    if (decision.action === "landing") {
      throw redirect({
        href: await loadLandingTarget(context.queryClient, context.apiClient, parsed.next),
      });
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
 * shows the edition's card, falling back to the community entry card with
 * the same return-path handling until the card slice fills the slot; every
 * other web build never renders this (the guard forwarded already).
 */
function LoginScreen(): React.ReactElement | null {
  const search = parseLoginSearch(Route.useSearch());
  if (platform.kind === "desktop") {
    return <DesktopSignInScreen error={search.error} errorDescription={search.errorDescription} />;
  }
  if (!rendersLoginInPlace(edition)) return null;
  const CloudLoginCard = edition.slots?.cloudLoginCard;
  if (CloudLoginCard !== undefined) return <CloudLoginCard />;
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
}
