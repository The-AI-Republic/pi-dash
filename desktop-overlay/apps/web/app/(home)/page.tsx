/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * Desktop root route (`/`). Also serves `/login` and `/sign-in` via
 * re-exports under app/routes/redirects/core/.
 *
 * Whatever an edition's web build does at `/` (a marketing page, a full
 * sign-in screen), the desktop wrapper has no such surface: signed-out users
 * get the sign-in card directly — no header, no intro copy. Signed-in users
 * are forwarded to their workspace (honouring `?next_path=`) by
 * AuthenticationWrapper.
 *
 * The card comes from the edition seam
 * `@/pi-dash-web/components/desktop/sign-in-card`. It must never start
 * sign-in on its own: on desktop that can mean opening the system browser,
 * which must stay a click.
 *
 * `?error=` / `?error_description=` arrive here from two places — the
 * server's `desktop-exchange` failure redirect (`/sign-in?error=…`, bounced
 * to the bundle by main.rs) and the deep-link handler when the identity
 * provider reports a denied/aborted flow — and are shown above the card.
 */

import { useSearchParams } from "next/navigation";
import { EPageTypes } from "@/helpers/authentication.helper";
import DefaultLayout from "@/layouts/default-layout";
import { AuthenticationWrapper } from "@/lib/wrappers/authentication-wrapper";
import { DesktopSignInCard } from "@/pi-dash-web/components/desktop/sign-in-card";

export const meta = () => [{ title: "Sign in — Pi Dash" }];

function SignInError() {
  const searchParams = useSearchParams();
  const error = searchParams.get("error");
  if (!error) return null;
  const description = searchParams.get("error_description");

  return (
    <div
      role="alert"
      className="w-full max-w-md rounded-lg border border-danger-strong bg-danger-subtle px-4 py-3 text-body-sm-regular text-danger-primary"
    >
      <p className="text-body-sm-semibold">Sign-in didn&apos;t complete</p>
      <p>{description || `Please try again. (${error})`}</p>
    </div>
  );
}

function DesktopSignIn() {
  return (
    <div className="flex h-full w-full flex-col items-center justify-center gap-4 bg-canvas px-6">
      <SignInError />
      <DesktopSignInCard />
    </div>
  );
}

export default function DesktopRootPage() {
  return (
    <DefaultLayout>
      <AuthenticationWrapper pageType={EPageTypes.NON_AUTHENTICATED}>
        <DesktopSignIn />
      </AuthenticationWrapper>
    </DefaultLayout>
  );
}
