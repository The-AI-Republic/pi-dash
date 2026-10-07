/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

// types
import type { ICsrfTokenData } from "@pi-dash/types";
import { isDesktop } from "@/services/agent-runtime";
import { clearDesktopSessionData } from "@/services/desktop-session";

/**
 * The minimal slice of ``AuthService`` the sign-out strategy needs. Declared
 * structurally (not the concrete class) so an alternate edition can supply a
 * different strategy without depending on the whole service.
 */
export interface SignOutClient {
  requestCSRFToken(): Promise<ICsrfTokenData>;
  post(url: string, data?: unknown): Promise<unknown>;
}

// Mirrors the Rust side's error string (desktop_http.rs).
const REDIRECT_REFUSED = "API redirect is not allowed";

/**
 * Desktop sign-out. The window never navigates to the server host outside
 * `/api/` (the app bounces such navigations back to the bundle), so the form
 * POST below would never reach the server. Send the same request over the API
 * transport instead, then clear the app's own cookie jar — the request needs
 * the CSRF cookie, so the order matters — and land on the bundled sign-in page.
 */
async function performDesktopSignOut(client: SignOutClient, csrfToken: string): Promise<void> {
  // The view answers a completed sign-out with a redirect to the hosted web
  // app, which the native transport refuses to follow — that refusal is the
  // success signal. Anything else (a network failure, or the CSRF failure
  // page, which is served with a 200) means the session is still alive on the
  // server, so stay signed in.
  const signedOut = await client.post("/auth/sign-out/", new URLSearchParams({ csrfmiddlewaretoken: csrfToken })).then(
    () => false,
    (error) => {
      if (error instanceof Error && error.message.includes(REDIRECT_REFUSED)) return true;
      throw error;
    }
  );
  if (!signedOut) throw new Error("Sign-out was not completed by the server");
  await clearDesktopSessionData();
  // Full navigation (not a router push) so every store starts from scratch.
  window.location.replace("/");
}

/**
 * Default (self-hosted) sign-out: fetch a CSRF token and submit a hidden form
 * POST to Django's ``/auth/sign-out/`` session-logout endpoint, which clears
 * the session cookie and redirects.
 *
 * This is the overridable seam for downstream editions (e.g. a hosted OIDC
 * edition whose logout is a JSON request that returns an upstream logout URL).
 * Editions override sign-out by replacing this file wholesale — keep the
 * exported ``performSignOut`` name and signature stable so ``AuthService``
 * keeps resolving it.
 */
export async function performSignOut(client: SignOutClient, baseUrl: string): Promise<void> {
  const data = await client.requestCSRFToken();
  const csrfToken = data?.csrf_token;

  if (!csrfToken) throw new Error("CSRF token not found");

  if (isDesktop()) return performDesktopSignOut(client, csrfToken);

  const form = document.createElement("form");
  const input = document.createElement("input");

  form.method = "POST";
  form.action = `${baseUrl}/auth/sign-out/`;

  input.value = csrfToken;
  input.name = "csrfmiddlewaretoken";
  input.type = "hidden";
  form.appendChild(input);

  document.body.appendChild(form);

  form.submit();
}
