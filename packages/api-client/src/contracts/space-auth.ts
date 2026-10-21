// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Spaces sign-in (parity/specs/space.md §5.4): the email check that routes
// the card between password and magic-code steps, magic-code generation,
// and the native Django form endpoints the card posts to (sign-in, sign-up,
// magic sign-in/up, sign-out). Those form views answer a redirect — success
// to the validated return path, failure back to the spaces base with
// `error_code`/`error_message` query parameters. The CSRF read is shared
// with the workspace entry flow (`getCsrfToken`).
import * as z from "zod/mini";
import { ApiClient } from "../client.js";

/** Spaces base path the auth views redirect back to. */
export const SPACE_BASE_PATH = "/spaces/";

/** Which credential the card asks for next. */
export const SpaceEmailCheck = z.object({
  existing: z.boolean(),
  status: z.enum(["MAGIC_CODE", "CREDENTIAL"]),
});
export type SpaceEmailCheck = z.infer<typeof SpaceEmailCheck>;

/** Error envelope the JSON spaces-auth endpoints answer on refusal. */
export const SpaceAuthError = z.object({
  error_code: z.union([z.number(), z.string()]),
  error_message: z.string(),
});
export type SpaceAuthError = z.infer<typeof SpaceAuthError>;

/** Response of the magic-code generation call: the key the code is stored under. */
export const SpaceMagicGenerate = z.object({
  key: z.string(),
});
export type SpaceMagicGenerate = z.infer<typeof SpaceMagicGenerate>;

/**
 * Auth refusal codes the spaces card models, by name, with the server's
 * numeric code beside each. The card maps the number to a step and a
 * banner; it never shows the number. Rate limiting (5900) and a Gitea
 * configuration failure (5112) are deliberately absent — see
 * parity/specs/space.md §5.4.
 */
export const SPACE_AUTH_ERROR_CODES = {
  INVALID_EMAIL: 5005,
  EMAIL_REQUIRED: 5010,
  SIGNUP_DISABLED: 5015,
  USER_ACCOUNT_DEACTIVATED: 5019,
  INVALID_PASSWORD: 5020,
  PASSWORD_TOO_WEAK: 5021,
  SMTP_NOT_CONFIGURED: 5025,
  USER_ALREADY_EXIST: 5030,
  AUTHENTICATION_FAILED_SIGN_UP: 5035,
  REQUIRED_EMAIL_PASSWORD_SIGN_UP: 5040,
  INVALID_EMAIL_SIGN_UP: 5045,
  INVALID_EMAIL_MAGIC_SIGN_UP: 5050,
  MAGIC_SIGN_UP_EMAIL_CODE_REQUIRED: 5055,
  USER_DOES_NOT_EXIST: 5060,
  AUTHENTICATION_FAILED_SIGN_IN: 5065,
  REQUIRED_EMAIL_PASSWORD_SIGN_IN: 5070,
  INVALID_EMAIL_SIGN_IN: 5075,
  INVALID_EMAIL_MAGIC_SIGN_IN: 5080,
  MAGIC_SIGN_IN_EMAIL_CODE_REQUIRED: 5085,
  INVALID_MAGIC_CODE_SIGN_IN: 5090,
  INVALID_MAGIC_CODE_SIGN_UP: 5092,
  EXPIRED_MAGIC_CODE_SIGN_IN: 5095,
  EXPIRED_MAGIC_CODE_SIGN_UP: 5097,
  EMAIL_CODE_ATTEMPT_EXHAUSTED_SIGN_IN: 5100,
  EMAIL_CODE_ATTEMPT_EXHAUSTED_SIGN_UP: 5102,
  OAUTH_NOT_CONFIGURED: 5104,
  GOOGLE_NOT_CONFIGURED: 5105,
  GITHUB_NOT_CONFIGURED: 5110,
  GITLAB_NOT_CONFIGURED: 5111,
  GOOGLE_OAUTH_PROVIDER_ERROR: 5115,
  GITHUB_OAUTH_PROVIDER_ERROR: 5120,
  GITLAB_OAUTH_PROVIDER_ERROR: 5121,
  GITHUB_USER_NOT_IN_ORG: 5122,
  INSTANCE_NOT_CONFIGURED: 5000,
} as const;

export type SpaceAuthErrorCode = keyof typeof SPACE_AUTH_ERROR_CODES;

/** Result of a form post: the redirect target, split into success/refusal. */
export type SpaceAuthResult =
  | { ok: true; location: string }
  | { ok: false; location: string; code: string; message: string };

function readQuery(location: string, name: string): string | null {
  const separator = location.indexOf("?");
  if (separator < 0) return null;
  return new URLSearchParams(location.slice(separator + 1)).get(name);
}

/** Reads the error-code convention off a spaces-auth redirect target. */
export function parseSpaceAuthRedirect(location: string): SpaceAuthResult {
  const code = location ? readQuery(location, "error_code") : null;
  if (code) {
    return { ok: false, location, code, message: readQuery(location, "error_message") ?? code };
  }
  return { ok: true, location };
}

/** POST /auth/spaces/email-check/ with `{ existing, status }`. */
export async function checkSpaceEmail(client: ApiClient, email: string): Promise<SpaceEmailCheck> {
  const path = "/auth/spaces/email-check/";
  const envelope = await client.sendJson<unknown>("POST", path, { json: { email } });
  return client.parse(SpaceEmailCheck, envelope.data, `POST ${path}`);
}

/** POST /auth/spaces/magic-generate/ with `{ key }`. */
export async function generateSpaceMagicCode(client: ApiClient, email: string): Promise<SpaceMagicGenerate> {
  const path = "/auth/spaces/magic-generate/";
  const envelope = await client.sendJson<unknown>("POST", path, { json: { email } });
  return client.parse(SpaceMagicGenerate, envelope.data, `POST ${path}`);
}

async function spaceFormPost(
  client: ApiClient,
  path: string,
  form: Record<string, string | undefined>
): Promise<SpaceAuthResult> {
  const response = await client.post(path, { form });
  const result = parseSpaceAuthRedirect(response.url);
  // A successful sign-in/sign-up rotates CSRF; the next unsafe request
  // must fetch a fresh token (same rule as the workspace flows).
  if (result.ok) client.clearCsrfCache();
  return result;
}

export interface SpaceSignInInput {
  email: string;
  password: string;
  nextPath?: string;
}

export async function signInToSpace(client: ApiClient, input: SpaceSignInInput): Promise<SpaceAuthResult> {
  return spaceFormPost(client, "/auth/spaces/sign-in/", {
    email: input.email,
    password: input.password,
    next_path: input.nextPath,
  });
}

export interface SpaceSignUpInput {
  email: string;
  password: string;
  confirmPassword?: string;
  nextPath?: string;
}

export async function signUpToSpace(client: ApiClient, input: SpaceSignUpInput): Promise<SpaceAuthResult> {
  return spaceFormPost(client, "/auth/spaces/sign-up/", {
    email: input.email,
    password: input.password,
    confirm_password: input.confirmPassword,
    next_path: input.nextPath,
  });
}

export interface SpaceMagicSignInInput {
  email: string;
  code: string;
  nextPath?: string;
}

export async function signInToSpaceWithMagicCode(
  client: ApiClient,
  input: SpaceMagicSignInInput
): Promise<SpaceAuthResult> {
  return spaceFormPost(client, "/auth/spaces/magic-sign-in/", {
    email: input.email,
    code: input.code,
    next_path: input.nextPath,
  });
}

export interface SpaceMagicSignUpInput {
  email: string;
  code: string;
  nextPath?: string;
}

export async function signUpToSpaceWithMagicCode(
  client: ApiClient,
  input: SpaceMagicSignUpInput
): Promise<SpaceAuthResult> {
  return spaceFormPost(client, "/auth/spaces/magic-sign-up/", {
    email: input.email,
    code: input.code,
    next_path: input.nextPath,
  });
}

/** Ends the visitor's session; the view redirects to the return path. */
export async function signOutOfSpace(client: ApiClient, nextPath?: string): Promise<SpaceAuthResult> {
  return spaceFormPost(client, "/auth/spaces/sign-out/", { next_path: nextPath });
}
