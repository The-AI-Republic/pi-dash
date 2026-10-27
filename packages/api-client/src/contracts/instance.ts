// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Instance-admin (god-mode) contracts: renaming the instance, the admin
// session check and admin list, the native admin auth form posts, the
// shared configurations key map, the email disable/test calls, and the
// cloud one-shot auth probe. The god-mode screens authenticate through
// their own admin session, separate from the member session.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { isApiError } from "../errors.js";
import { SignOutResult } from "./auth.js";
import { InstanceDetails, SignInResult, parseAuthRedirectResult } from "./auth-entry.js";
import { dateTime, nullableDateTime, nullableUuid, uuid } from "./fields.js";

export interface InstancePatch {
  instance_name?: string;
  is_telemetry_enabled?: boolean;
}

/**
 * PATCH /api/instances/ renames the instance and flips telemetry in one
 * write (the general page saves both together). Answers the stored row;
 * the bootstrap-only workspace flag is not part of the answer.
 */
export async function updateInstance(client: ApiClient, patch: InstancePatch): Promise<InstanceDetails> {
  const envelope = await client.sendJson<unknown>("PATCH", "/api/instances/", { json: patch });
  return client.parse(InstanceDetails, envelope.data, "PATCH /api/instances/");
}

/** The admin session as the admins/me check reports it. */
export const AdminMe = z.object({
  id: uuid(),
  avatar: z.string(),
  avatar_url: z.nullable(z.string()),
  cover_image: z.nullable(z.string()),
  date_joined: dateTime(),
  display_name: z.string(),
  email: z.email(),
  first_name: z.string(),
  last_name: z.string(),
  is_active: z.boolean(),
  is_bot: z.boolean(),
  is_email_verified: z.boolean(),
  user_timezone: z.string(),
  username: z.string(),
  is_password_autoset: z.boolean(),
});
export type AdminMe = z.infer<typeof AdminMe>;

export type AdminSession = { kind: "admin"; admin: AdminMe } | { kind: "anonymous" } | { kind: "forbidden" };

/**
 * GET /api/instances/admins/me/ drives the god-mode login tri-state: an
 * admin session parses, 401 reads as anonymous (a member session reads
 * as anonymous too — admin paths use a separate cookie), and 403 means
 * authentication is not done (the grant was revoked mid-session). Any
 * other failure (5xx, network) throws so callers can tell a transient
 * outage from a verdict and offer a retry instead of a redirect.
 */
export async function getAdminSession(client: ApiClient): Promise<AdminSession> {
  try {
    const body = await client.getJson<unknown>("/api/instances/admins/me/");
    return { kind: "admin", admin: client.parse(AdminMe, body, "GET /api/instances/admins/me/") };
  } catch (error) {
    if (isApiError(error) && (error.status === 401 || error.status === 403)) {
      return error.status === 401 ? { kind: "anonymous" } : { kind: "forbidden" };
    }
    throw error;
  }
}

/** The nested user projection each admins-list row carries. */
export const InstanceAdminUser = z.object({
  id: uuid(),
  first_name: z.string(),
  last_name: z.string(),
  avatar: z.string(),
  avatar_url: z.nullable(z.string()),
  is_bot: z.boolean(),
  display_name: z.string(),
  email: z.email(),
  last_login_medium: z.string(),
});
export type InstanceAdminUser = z.infer<typeof InstanceAdminUser>;

/** One row of the admins list, with its nested user projection. */
export const InstanceAdminEntry = z.object({
  id: uuid(),
  created_at: dateTime(),
  updated_at: dateTime(),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
  deleted_at: nullableDateTime(),
  user: uuid(),
  instance: uuid(),
  role: z.number(),
  is_verified: z.boolean(),
  user_detail: InstanceAdminUser,
});
export type InstanceAdminEntry = z.infer<typeof InstanceAdminEntry>;

export const InstanceAdminList = z.array(InstanceAdminEntry);
export type InstanceAdminList = z.infer<typeof InstanceAdminList>;

/**
 * GET /api/instances/admins/ lists the instance admins. The general
 * page reads the first entry's nested user email into its read-only
 * admin field.
 */
export async function listInstanceAdmins(client: ApiClient): Promise<InstanceAdminList> {
  const body = await client.getJson<unknown>("/api/instances/admins/");
  return client.parse(InstanceAdminList, body, "GET /api/instances/admins/");
}

export interface AdminSignUpInput {
  firstName: string;
  lastName?: string;
  email: string;
  companyName?: string;
  password: string;
  confirmPassword?: string;
  telemetryEnabled?: boolean;
}

/**
 * The first-run setup post. The first user becomes the instance admin:
 * the server creates the admin, marks the instance set up, stores the
 * company on the profile, adopts it as the instance name, applies the
 * telemetry flag, and redirects to general settings. Failures redirect
 * back carrying error_code/error_message plus the echoed fields, read
 * through the shared redirect convention. The form submits the confirm
 * field the server ignores (matching is enforced client-side) and the
 * telemetry flag as "True"/"False".
 */
export async function adminSignUp(client: ApiClient, input: AdminSignUpInput): Promise<SignInResult> {
  const response = await client.post("/api/instances/admins/sign-up/", {
    form: {
      first_name: input.firstName,
      last_name: input.lastName ?? "",
      email: input.email,
      company_name: input.companyName ?? "",
      password: input.password,
      confirm_password: input.confirmPassword ?? input.password,
      is_telemetry_enabled: (input.telemetryEnabled ?? true) ? "True" : "False",
    },
  });
  // Account creation logs the session in, rotating CSRF like signIn.
  client.clearCsrfCache();
  return parseAuthRedirectResult(response.url);
}

export interface AdminSignInInput {
  email: string;
  password: string;
}

/**
 * The admin sign-in post. Success establishes the admin session cookie
 * and lands on general settings; failures redirect back carrying
 * error_code/error_message, read through the shared convention. A valid
 * password for a non-admin fails exactly like a wrong password.
 */
export async function adminSignIn(client: ApiClient, input: AdminSignInInput): Promise<SignInResult> {
  const response = await client.post("/api/instances/admins/sign-in/", {
    form: {
      email: input.email,
      password: input.password,
    },
  });
  // A successful login rotates the CSRF secret (see signIn).
  client.clearCsrfCache();
  return parseAuthRedirectResult(response.url);
}

/**
 * The admin sign-out post. The server clears the session and redirects
 * to the mount; the member session is untouched.
 */
export async function adminSignOut(client: ApiClient): Promise<SignOutResult> {
  const response = await client.post("/api/instances/admins/sign-out/", {});
  // The session is gone; its CSRF token goes with it.
  client.clearCsrfCache();
  return { location: response.url };
}

/**
 * One row of the configurations key map. Every value is a string, with
 * boolean flags encoded as "1"/"0". `source` tells where the key is
 * actually read from: env-sourced rows are managed outside the database
 * and render read-only.
 */
export const InstanceConfiguration = z.object({
  id: uuid(),
  created_at: dateTime(),
  updated_at: dateTime(),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
  deleted_at: nullableDateTime(),
  key: z.string(),
  value: z.nullable(z.string()),
  category: z.string(),
  is_encrypted: z.boolean(),
  source: z.enum(["db", "env"]),
  is_managed: z.boolean(),
});
export type InstanceConfiguration = z.infer<typeof InstanceConfiguration>;

export const InstanceConfigurationList = z.array(InstanceConfiguration);
export type InstanceConfigurationList = z.infer<typeof InstanceConfigurationList>;

/**
 * GET /api/instances/configurations/ reads the whole key map in one
 * fetch, shared by every settings page.
 */
export async function listInstanceConfigurations(client: ApiClient): Promise<InstanceConfigurationList> {
  const body = await client.getJson<unknown>("/api/instances/configurations/");
  return client.parse(InstanceConfigurationList, body, "GET /api/instances/configurations/");
}

/**
 * PATCH /api/instances/configurations/ writes a partial string map and
 * answers the touched rows, which saved forms re-read so they turn
 * clean. Only keys that already have rows are touched; unknown keys
 * are silently ignored, never created.
 */
export async function updateInstanceConfigurations(
  client: ApiClient,
  patch: Record<string, string>
): Promise<InstanceConfigurationList> {
  const envelope = await client.sendJson<unknown>("PATCH", "/api/instances/configurations/", { json: patch });
  return client.parse(InstanceConfigurationList, envelope.data, "PATCH /api/instances/configurations/");
}

/**
 * DELETE /api/instances/configurations/disable-email-feature/ turns the
 * master email toggle off: it zeroes the SMTP-enabled flag and blanks
 * the stored mail values (host, port, sender, user, password) while
 * leaving the transport-security pair untouched.
 */
export async function disableEmailFeature(client: ApiClient): Promise<number> {
  const response = await client.delete("/api/instances/configurations/disable-email-feature/");
  return response.status;
}

/** The mailed test-message confirmation. */
export const EmailCredentialCheckResult = z.object({
  message: z.string(),
});
export type EmailCredentialCheckResult = z.infer<typeof EmailCredentialCheckResult>;

/**
 * SMTP values a caller may grade instead of the stored configuration.
 * Fields are optional: anything left out (or null) is graded against
 * the stored setting, and a blank password also means "use the stored
 * one", since a stored secret never reads back. Security is the
 * single TLS/SSL/none choice, which the contract fans out to the two
 * flag keys the endpoint understands.
 */
export interface EmailCredentialCandidate {
  host?: string | null | undefined;
  port?: string | null | undefined;
  from?: string | null | undefined;
  user?: string | null | undefined;
  password?: string | null | undefined;
  security?: "tls" | "ssl" | "none" | null | undefined;
}

/**
 * POST /api/instances/email-credentials-check/ sends a real message to
 * the receiver and reports the outcome. Without ``candidate`` the send
 * is graded against the stored SMTP configuration; with it, the
 * entered (possibly unsaved) values are graded instead, so "test before
 * save" tests what the form shows. Failures surface as the server's
 * error message on a 400.
 */
export async function checkEmailCredentials(
  client: ApiClient,
  receiverEmail: string,
  candidate?: EmailCredentialCandidate
): Promise<EmailCredentialCheckResult> {
  const envelope = await client.sendJson<unknown>("POST", "/api/instances/email-credentials-check/", {
    json: { receiver_email: receiverEmail, ...candidatePayload(candidate) },
  });
  return client.parse(EmailCredentialCheckResult, envelope.data, "POST /api/instances/email-credentials-check/");
}

/**
 * Wire shape for the candidate settings: the endpoint's configuration
 * keys, with the security choice spelled as the two flag keys. Blank
 * values are omitted so the endpoint keeps the stored setting.
 */
export function candidatePayload(candidate: EmailCredentialCandidate | undefined): Record<string, string> {
  if (!candidate) return {};
  const payload: Record<string, string> = {};
  const put = (key: string, value: string | null | undefined) => {
    if (value === null || value === undefined) return;
    if (value.trim() === "") return;
    payload[key] = value;
  };
  put("EMAIL_HOST", candidate.host);
  put("EMAIL_PORT", candidate.port);
  put("EMAIL_FROM", candidate.from);
  put("EMAIL_HOST_USER", candidate.user);
  put("EMAIL_HOST_PASSWORD", candidate.password);
  if (candidate.security !== null && candidate.security !== undefined) {
    payload["EMAIL_USE_TLS"] = candidate.security === "tls" ? "1" : "0";
    payload["EMAIL_USE_SSL"] = candidate.security === "ssl" ? "1" : "0";
  }
  return payload;
}

/**
 * The cloud identity payload. The probe classifies; the payload rides
 * along lightly, so the schema stays shallow on purpose.
 */
export const CloudAuthUser = z.object({
  id: z.string(),
  email: z.string(),
  display_name: z.string(),
  first_name: z.string(),
  last_name: z.string(),
  avatar: z.string(),
});
export type CloudAuthUser = z.infer<typeof CloudAuthUser>;

export type CloudAuthProbe =
  | { kind: "authenticated"; user: CloudAuthUser }
  | { kind: "anonymous" }
  | { kind: "inconclusive" };

/**
 * The cloud entry's one-shot probe: a credentialed read of
 * GET /api/auth/me/ that fires only on a genuine admin-check 403. A 200
 * means the visitor is signed in, 401/403 means no session, and any
 * other status, 5xx, or network failure is inconclusive — never guessed
 * anonymous, so a flaky probe cannot send a visitor into an
 * OIDC-bounce loop. Only contract violations and caller aborts throw.
 */
export async function probeCloudAuthSession(client: ApiClient): Promise<CloudAuthProbe> {
  try {
    const body = await client.getJson<unknown>("/api/auth/me/");
    return { kind: "authenticated", user: client.parse(CloudAuthUser, body, "GET /api/auth/me/") };
  } catch (error) {
    if (isApiError(error)) {
      if (error.status === 401 || error.status === 403) return { kind: "anonymous" };
      if (error.code !== "contract" && error.code !== "aborted") return { kind: "inconclusive" };
    }
    throw error;
  }
}

/**
 * The cloud OIDC entry with its deep-link return: the god-mode root
 * bounces no-session visitors here carrying the current path plus
 * query as a relative `return_to`, so the round trip lands back on
 * the same URL. Built against the API base (or the window origin when
 * unconfigured); the full-page redirect replaces history.
 */
export function cloudSignInUrl(apiBase: string, returnTo: string): string {
  const base = apiBase.replace(/\/+$/, "");
  return `${base}/api/auth/sign-in/?return_to=${encodeURIComponent(returnTo)}`;
}
