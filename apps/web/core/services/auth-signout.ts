/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

// types
import type { ICsrfTokenData } from "@pi-dash/types";

/**
 * The minimal slice of ``AuthService`` the sign-out strategy needs. Declared
 * structurally (not the concrete class) so an alternate edition can supply a
 * different strategy without depending on the whole service.
 */
export interface SignOutClient {
  requestCSRFToken(): Promise<ICsrfTokenData>;
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
