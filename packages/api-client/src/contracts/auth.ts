// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Auth contracts. The backend signs users in with a session cookie: the
// client fetches a CSRF token, posts credentials as a form, and follows the
// redirect the server answers with. A redirect whose final URL carries
// error_code / error_message query params is a failed sign-in.
import * as z from "zod/mini";
import { ApiClient, CSRF_ENDPOINT } from "../client.js";

export const CsrfTokenResponse = z.object({
  csrf_token: z.string(),
});
export type CsrfTokenResponse = z.infer<typeof CsrfTokenResponse>;

export async function getCsrfToken(client: ApiClient): Promise<CsrfTokenResponse> {
  const body = await client.getJson<unknown>(CSRF_ENDPOINT, { csrf: false });
  return client.parse(CsrfTokenResponse, body, "GET /auth/get-csrf-token/");
}

export interface SignInInput {
  email: string;
  password: string;
  nextPath?: string;
}

export type SignInResult =
  | { ok: true; location: string }
  | { ok: false; location: string; code: string; message: string };

function firstQueryParam(url: string, name: string): string | null {
  const hash = url.indexOf("?");
  if (hash < 0) return null;
  const params = new URLSearchParams(url.slice(hash + 1));
  return params.get(name);
}

/**
 * The native auth views answer form POSTs with a redirect: success lands
 * on the app (or next_path), failure lands back carrying error_code and
 * error_message query params. Every form flow below reads that convention.
 */
function readRedirectResult(location: string): SignInResult {
  const code = location ? firstQueryParam(location, "error_code") : null;
  if (code) {
    return {
      ok: false,
      location,
      code,
      message: firstQueryParam(location, "error_message") ?? code,
    };
  }
  return { ok: true, location };
}

export async function signIn(client: ApiClient, input: SignInInput): Promise<SignInResult> {
  const response = await client.post("/auth/sign-in/", {
    form: {
      email: input.email,
      password: input.password,
      next_path: input.nextPath ?? undefined,
    },
  });
  // A successful login rotates the CSRF secret; drop the cached token so
  // the next unsafe request fetches a post-rotation one. The stale token
  // answers a bespoke 200 failure page, not a 403, so nothing downstream
  // can detect it — the refresh must happen here.
  client.clearCsrfCache();
  return readRedirectResult(response.url);
}

/**
 * Which credential the sign-in card asks for next. Answered by
 * POST /auth/email-check/ with { existing, status }: an unknown address
 * still reports its mode so the card can route into sign-up later.
 */
export const EmailCheckResponse = z.object({
  existing: z.boolean(),
  status: z.enum(["MAGIC_CODE", "CREDENTIAL"]),
});
export type EmailCheckResponse = z.infer<typeof EmailCheckResponse>;

export async function checkEmail(client: ApiClient, email: string): Promise<EmailCheckResponse> {
  const envelope = await client.sendJson<unknown>("POST", "/auth/email-check/", { json: { email } });
  return client.parse(EmailCheckResponse, envelope.data, "POST /auth/email-check/");
}

/** Response of POST /auth/magic-generate/: the key the code was stored under. */
export const MagicGenerateResponse = z.object({
  key: z.string(),
});
export type MagicGenerateResponse = z.infer<typeof MagicGenerateResponse>;

export async function generateMagicCode(client: ApiClient, email: string): Promise<MagicGenerateResponse> {
  const envelope = await client.sendJson<unknown>("POST", "/auth/magic-generate/", { json: { email } });
  return client.parse(MagicGenerateResponse, envelope.data, "POST /auth/magic-generate/");
}

export interface MagicSignInInput {
  email: string;
  code: string;
  nextPath?: string;
}

export async function signInWithMagicCode(client: ApiClient, input: MagicSignInInput): Promise<SignInResult> {
  const response = await client.post("/auth/magic-sign-in/", {
    form: {
      email: input.email,
      code: input.code,
      next_path: input.nextPath ?? undefined,
    },
  });
  // Same rotation as password sign-in (see signIn).
  client.clearCsrfCache();
  return readRedirectResult(response.url);
}

export interface SignOutResult {
  location: string;
}

export async function signOut(client: ApiClient): Promise<SignOutResult> {
  const response = await client.post("/auth/sign-out/", {});
  // The session is gone; its CSRF token goes with it.
  client.clearCsrfCache();
  return { location: response.url };
}
