// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Session-expired redirect (AUTH-020). Any 401 already moves the session
// store to expired (see core/session/middleware); this effect notices once
// and sends the user to the sign-in entry (/) with the return path
// preserved as next_path (/sign-in is a retired bare redirect that drops
// params, so it cannot carry the return path). The app shell and every
// authenticated surface outside it (the set-password page) share it.

import { useRouter } from "@tanstack/react-router";
import * as React from "react";

import { useSessionStore } from "../../core/session/store.js";
import { notifyError } from "./toaster.js";

/**
 * Addresses a 401 must never bounce away from (CLOUD-013/CLOUD-015, from
 * parity/specs/cloud.md §3–§4): the root, the marketing pages, the login
 * page and its legacy aliases, sign-up in both spellings, and the
 * account-recovery pair. Each matches with or without a trailing slash.
 */
const BOUNCE_EXEMPT_PATHS = new Set([
  "/",
  "/home",
  "/docs",
  "/downloads",
  "/pricing",
  "/login",
  "/sign-in",
  "/signin",
  "/sign-up",
  "/accounts/sign-up",
  "/accounts/forgot-password",
  "/accounts/reset-password",
]);

/** The pathname of a router address: no query, no fragment, no trailing slash. */
export function bouncePathname(href: string): string {
  return href.replace(/[?#].*$/, "").replace(/\/+$/, "") || "/";
}

/** True on the public and auth addresses an expired session leaves alone. */
export function isBounceExempt(href: string): boolean {
  return BOUNCE_EXEMPT_PATHS.has(bouncePathname(href));
}

export function useSessionExpiredRedirect(): void {
  const router = useRouter();
  const status = useSessionStore((state) => state.status);
  const returnUrl = useSessionStore((state) => state.returnUrl);
  const expiredNoticed = React.useRef(false);

  React.useEffect(() => {
    if (status !== "expired" || expiredNoticed.current) return;
    const current = router.state.location.href;
    // An exempt page stays put (and stays quiet): the bounce fires on the
    // next expiry past one, once the user is somewhere that needs a session.
    if (isBounceExempt(current)) return;
    expiredNoticed.current = true;
    notifyError("Session expired", "Sign in again to continue.");
    void router.navigate({ to: "/", search: { next_path: returnUrl ?? current } });
  }, [status, returnUrl, router]);
}
