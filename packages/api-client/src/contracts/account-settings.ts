// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Account-security writes: the two-step email change and the password
// change. Both work on the signed-in member only; deactivation lives in
// auth.ts and the CSRF read in auth-entry.ts.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { Me } from "./users.js";

/**
 * The server's acknowledgement of a one-time code send. The mail itself
 * is fire-and-forget; only this message comes back.
 */
export const EmailCodeResult = z.object({
  message: z.string(),
});
export type EmailCodeResult = z.infer<typeof EmailCodeResult>;

/**
 * POST /api/users/me/email/generate-code/ with the new address. Refusals
 * (empty, malformed, unchanged or taken address) answer `{ error }` with
 * a 400, so callers surface `ApiError.message`.
 */
export async function generateEmailChangeCode(client: ApiClient, email: string): Promise<EmailCodeResult> {
  const path = "/api/users/me/email/generate-code/";
  const envelope = await client.sendJson<unknown>("POST", path, { json: { email } });
  return client.parse(EmailCodeResult, envelope.data, `POST ${path}`);
}

/**
 * PATCH /api/users/me/email/ verifies the mailed code and moves the
 * account. Answers the full user row; the server ends the session as a
 * side effect, so callers clear cached auth state afterwards.
 */
export async function changeEmail(client: ApiClient, email: string, code: string): Promise<Me> {
  const path = "/api/users/me/email/";
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: { email, code } });
  const updated = client.parse(Me, envelope.data, `PATCH ${path}`);
  client.clearCsrfCache();
  return updated;
}

/** POST /auth/change-password/ success acknowledgement. */
export const PasswordChangeResult = z.object({
  message: z.string(),
});
export type PasswordChangeResult = z.infer<typeof PasswordChangeResult>;

export interface PasswordChangeInput {
  oldPassword?: string;
  newPassword: string;
}

/**
 * POST /auth/change-password/ with the current secret (omitted for
 * provider-created accounts) and the new one. The client's CSRF handling
 * adds the forgery header; a weak or stale guess answers a 400 with an
 * error code the caller maps to a field.
 */
export async function changePassword(client: ApiClient, input: PasswordChangeInput): Promise<PasswordChangeResult> {
  const envelope = await client.sendJson<unknown>("POST", "/auth/change-password/", {
    json: { old_password: input.oldPassword, new_password: input.newPassword },
  });
  const result = client.parse(PasswordChangeResult, envelope.data, "POST /auth/change-password/");
  // A successful change rotates the session and the CSRF token, exactly
  // like sign-in, so the next write must fetch a fresh forgery token.
  client.clearCsrfCache();
  return result;
}
