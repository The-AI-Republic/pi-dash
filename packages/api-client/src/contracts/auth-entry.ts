// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Signed-out entry flows: the CSRF token read, password and magic-code
// sign-in and sign-up, the email check, password recovery and first
// password, CLI device approval, and the instance bootstrap read the
// entry cards need. Sign-out, deactivation and the session instance
// config stay in auth.ts; everything here runs from lazy routes, so it
// stays out of the initial bundle (NEWFRONT-523, size gate).
import * as z from "zod/mini";
import { ApiClient, CSRF_ENDPOINT } from "../client.js";
import { dateTime, nullableDateTime, nullableUuid, uuid } from "./fields.js";
import { InstanceConfig } from "./auth.js";

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

export function firstQueryParam(url: string, name: string): string | null {
  const hash = url.indexOf("?");
  if (hash < 0) return null;
  const params = new URLSearchParams(url.slice(hash + 1));
  return params.get(name);
}

/**
 * The native auth views answer form POSTs with a redirect: success lands
 * on the app (or next_path), failure lands back carrying error_code and
 * error_message query params. Every form flow below reads that convention,
 * and cards use this to read link-carried codes off their own URL.
 */
export function parseAuthRedirectResult(location: string): SignInResult {
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
  return parseAuthRedirectResult(response.url);
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
  return parseAuthRedirectResult(response.url);
}

export interface SignUpInput {
  email: string;
  password: string;
  confirmPassword?: string;
  nextPath?: string;
}

export async function signUp(client: ApiClient, input: SignUpInput): Promise<SignInResult> {
  const response = await client.post("/auth/sign-up/", {
    form: {
      email: input.email,
      password: input.password,
      confirm_password: input.confirmPassword ?? undefined,
      next_path: input.nextPath ?? undefined,
    },
  });
  // Account creation logs the session in, rotating CSRF like signIn.
  client.clearCsrfCache();
  return parseAuthRedirectResult(response.url);
}

export interface MagicSignUpInput {
  email: string;
  code: string;
  nextPath?: string;
}

export async function signUpWithMagicCode(client: ApiClient, input: MagicSignUpInput): Promise<SignInResult> {
  const response = await client.post("/auth/magic-sign-up/", {
    form: {
      email: input.email,
      code: input.code,
      next_path: input.nextPath ?? undefined,
    },
  });
  // Same rotation as password sign-up (see signUp).
  client.clearCsrfCache();
  return parseAuthRedirectResult(response.url);
}

/** Response of POST /auth/forgot-password/: the mailed-link confirmation. */
export const ForgotPasswordResponse = z.object({
  message: z.string(),
});
export type ForgotPasswordResponse = z.infer<typeof ForgotPasswordResponse>;

export async function requestPasswordReset(client: ApiClient, email: string): Promise<ForgotPasswordResponse> {
  const envelope = await client.sendJson<unknown>("POST", "/auth/forgot-password/", { json: { email } });
  return client.parse(ForgotPasswordResponse, envelope.data, "POST /auth/forgot-password/");
}

export interface ResetPasswordInput {
  uid: string;
  token: string;
  password: string;
  confirmPassword?: string;
}

export async function resetPassword(client: ApiClient, input: ResetPasswordInput): Promise<SignInResult> {
  const response = await client.post(
    `/auth/reset-password/${encodeURIComponent(input.uid)}/${encodeURIComponent(input.token)}/`,
    {
      form: {
        password: input.password,
        confirm_password: input.confirmPassword ?? undefined,
      },
    }
  );
  // The reset signs no session in, so the CSRF cache survives.
  return parseAuthRedirectResult(response.url);
}

/** Thin projection of the user row after the first password is set. */
export const SetPasswordResult = z.object({
  id: uuid(),
  email: z.email(),
  is_password_autoset: z.boolean(),
});
export type SetPasswordResult = z.infer<typeof SetPasswordResult>;

export async function setPassword(client: ApiClient, password: string): Promise<SetPasswordResult> {
  const envelope = await client.sendJson<unknown>("POST", "/auth/set-password/", { json: { password } });
  // Setting the first password re-logs the session in, rotating CSRF.
  client.clearCsrfCache();
  return client.parse(SetPasswordResult, envelope.data, "POST /auth/set-password/");
}

/**
 * Response of POST /api/v1/auth/device/start/: the grant the CLI polls.
 * `expires_in` and `interval` are seconds. Field names are the server's
 * protocol vocabulary; the client that runs the desktop CLI setup feeds the
 * device code back to the tool and approves the user code in the same
 * session.
 */
export const DeviceStartResponse = z.object({
  device_code: z.string(),
  user_code: z.string(),
  verification_uri: z.string(),
  expires_in: z.number(),
  interval: z.number(),
});
export type DeviceStartResponse = z.infer<typeof DeviceStartResponse>;

export async function startDeviceGrant(client: ApiClient): Promise<DeviceStartResponse> {
  const envelope = await client.sendJson<unknown>("POST", "/api/v1/auth/device/start/", { json: {} });
  return client.parse(DeviceStartResponse, envelope.data, "POST /api/v1/auth/device/start/");
}

/** Response of POST /api/v1/auth/device/approve/: the stamped grant. */
export const DeviceApproveResponse = z.object({
  ok: z.literal(true),
  user_email: z.email(),
  workspace_slug: z.nullable(z.string()),
});
export type DeviceApproveResponse = z.infer<typeof DeviceApproveResponse>;

export async function approveDeviceCode(client: ApiClient, userCode: string): Promise<DeviceApproveResponse> {
  const envelope = await client.sendJson<unknown>("POST", "/api/v1/auth/device/approve/", {
    json: { user_code: userCode },
  });
  return client.parse(DeviceApproveResponse, envelope.data, "POST /api/v1/auth/device/approve/");
}

/**
 * The stored instance row: identity, setup and telemetry flags, version
 * fields, and audit columns. PATCH answers exactly this object. (The
 * serializer declares one phantom nested field whose source column does
 * not exist; the backend drops it, so it is absent here too.)
 */
export const InstanceDetails = z.object({
  id: uuid(),
  created_at: dateTime(),
  updated_at: dateTime(),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
  deleted_at: nullableDateTime(),
  instance_name: z.string(),
  whitelist_emails: z.nullable(z.string()),
  instance_id: z.string(),
  current_version: z.string(),
  latest_version: z.nullable(z.string()),
  edition: z.string(),
  domain: z.string(),
  last_checked_at: dateTime(),
  namespace: z.nullable(z.string()),
  is_telemetry_enabled: z.boolean(),
  is_support_required: z.boolean(),
  is_setup_done: z.boolean(),
  is_signup_screen_visited: z.boolean(),
  is_verified: z.boolean(),
  is_test: z.boolean(),
  is_current_version_deprecated: z.boolean(),
});
export type InstanceDetails = z.infer<typeof InstanceDetails>;

/**
 * The `instance` half of the bootstrap read: the stored row plus the
 * workspace-existence flag the GET view annotates (it is computed, not
 * stored, so PATCH never returns it).
 */
export const InstanceInfo = z.extend(InstanceDetails, {
  workspaces_exist: z.boolean(),
});
export type InstanceInfo = z.infer<typeof InstanceInfo>;

/** The full bootstrap envelope: feature flags plus the instance row. */
export const InstanceBootstrap = z.object({
  config: InstanceConfig,
  instance: InstanceInfo,
});
export type InstanceBootstrap = z.infer<typeof InstanceBootstrap>;

export async function getInstanceBootstrap(client: ApiClient): Promise<InstanceBootstrap> {
  const body = await client.getJson<unknown>("/api/instances/");
  return client.parse(InstanceBootstrap, body, "GET /api/instances/");
}
