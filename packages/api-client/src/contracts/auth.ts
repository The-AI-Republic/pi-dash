// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
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

export async function signIn(client: ApiClient, input: SignInInput): Promise<SignInResult> {
  const response = await client.post("/auth/sign-in/", {
    form: {
      email: input.email,
      password: input.password,
      next_path: input.nextPath ?? undefined,
    },
  });
  const location = response.url;
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
