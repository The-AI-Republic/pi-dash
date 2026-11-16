// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud-edition session contracts: the user sign-in handoff URL, the
// session refresh plus the rejection verdict the refresh/replay
// interceptor reads, the cloud CSRF read and sign-out pair, the desktop
// deep-link code exchange, and the 402 quota-refusal shape the paywall
// interceptor reads. Shapes follow parity/specs/cloud.md and the CLOUD
// oracle rows, not old code. The cloud backend serves these paths; the
// OSS stack answers 404 (the contract suite pins the wiring there, and
// the area gate proves the cloud positives).
import * as z from "zod/mini";
import { ApiClient, CSRF_HEADER } from "../client.js";
import { isApiError } from "../errors.js";
import { CsrfTokenResponse, firstQueryParam } from "./auth-entry.js";

export interface CloudSignInHandoffInput {
  /** App-local path plus query the provider returns to; never a fragment. */
  returnTo: string;
  /** Browser origin; sent only when it differs from the auth origin. */
  returnOrigin?: string;
  /** Desktop shell: switch the endpoint to the desktop OIDC branch. */
  desktop?: boolean;
}

function sameOrigin(apiBase: string, origin: string): boolean {
  try {
    return new URL(origin).origin === new URL(apiBase).origin;
  } catch {
    return false;
  }
}

/**
 * The user sign-in handoff: GET /api/auth/sign-in/ carrying the return
 * target, the browser origin on split-origin setups only (same-origin
 * setups send no origin), and the desktop marker inside the desktop
 * shell. The god-mode builder in instance.ts is admin-only; this one is
 * the login-card, provider-button and desktop-card handoff. An origin
 * that cannot be compared is sent as-is for the server to judge.
 */
export function cloudSignInHandoffUrl(apiBase: string, input: CloudSignInHandoffInput): string {
  const base = apiBase.replace(/\/+$/, "");
  const params = [`return_to=${encodeURIComponent(input.returnTo)}`];
  if (input.returnOrigin && !sameOrigin(apiBase, input.returnOrigin)) {
    params.push(`return_origin=${encodeURIComponent(input.returnOrigin)}`);
  }
  if (input.desktop) {
    params.push("desktop=1");
  }
  return `${base}/api/auth/sign-in/?${params.join("&")}`;
}

/**
 * POST /api/auth/refresh/ rotates the cloud session cookie pair. No
 * body, no CSRF header — the refresh cookie alone authenticates it. A
 * 200 re-sets both cookies; anything else throws, and the interceptor
 * grades the failure with isCloudSessionRejected. Answers the status.
 */
export async function refreshCloudSession(client: ApiClient): Promise<number> {
  const response = await client.post("/api/auth/refresh/", { csrf: false });
  return response.status;
}

/**
 * Whether a refresh/replay failure is a definitive session rejection.
 * Only a 401/403 response says the session is dead and the user must
 * bounce to sign-in; transport failures, timeouts, aborts, contract
 * violations and every other status (including 5xx and throttle
 * answers) leave the page in place for a later retry.
 */
export function isCloudSessionRejected(error: unknown): boolean {
  return isApiError(error) && (error.status === 401 || error.status === 403);
}

/**
 * The cloud CSRF token path. The client's default CSRF plumbing reads
 * /auth/get-csrf-token/; cloud sign-out and the desktop agent read this
 * sibling path instead (same backend family, cloud path).
 */
export const CLOUD_CSRF_ENDPOINT = "/api/auth/get-csrf-token/";

export async function getCloudCsrfToken(client: ApiClient): Promise<CsrfTokenResponse> {
  const body = await client.getJson<unknown>(CLOUD_CSRF_ENDPOINT, { csrf: false });
  return client.parse(CsrfTokenResponse, body, "GET /api/auth/get-csrf-token/");
}

/**
 * The cloud logout answer: the upstream provider logout address. The
 * app deliberately never visits it — signing out of the app leaves the
 * provider session alone.
 */
export const CloudSignOutResponse = z.object({
  airepublic_logout_url: z.string(),
});
export type CloudSignOutResponse = z.infer<typeof CloudSignOutResponse>;

/**
 * POST /api/auth/sign-out/ ends the cloud app session. Reads a token
 * from the cloud CSRF path first (a missing token aborts the sign-out
 * with an error — nothing is posted without one), then posts an empty
 * JSON body with the token in the CSRF header. The server expires both
 * session cookies. The caller records the explicit-logout marker and
 * navigates to /login from scratch.
 */
export async function cloudSignOut(client: ApiClient): Promise<CloudSignOutResponse> {
  const csrf = await getCloudCsrfToken(client);
  const envelope = await client.sendJson<unknown>("POST", "/api/auth/sign-out/", {
    json: {},
    headers: { [CSRF_HEADER]: csrf.csrf_token },
  });
  // The session is gone; its CSRF token goes with it.
  client.clearCsrfCache();
  return client.parse(CloudSignOutResponse, envelope.data, "POST /api/auth/sign-out/");
}

export interface DesktopExchangeInput {
  code?: string;
  state?: string;
}

export type DesktopExchangeResult = { ok: true; location: string } | { ok: false; location: string; error: string };

/**
 * The exchange answers redirects, never JSON: success lands the webview
 * on the app, while refused flows (incomplete or unknown code/state)
 * land back on sign-in carrying a single `error` query param — a
 * different convention from the native error_code/error_message pair.
 */
export function parseDesktopExchangeResult(location: string): DesktopExchangeResult {
  const error = location ? firstQueryParam(location, "error") : null;
  if (error) {
    return { ok: false, location, error };
  }
  return { ok: true, location };
}

/**
 * GET /api/auth/desktop-exchange/ trades a deep-link code/state pair
 * for webview cookies. Called by the desktop shell outside the web
 * tree; the web side owns the handoff marker and surfaces the refusal.
 */
export async function exchangeDesktopCode(
  client: ApiClient,
  input: DesktopExchangeInput
): Promise<DesktopExchangeResult> {
  const response = await client.get("/api/auth/desktop-exchange/", {
    query: { code: input.code, state: input.state },
  });
  return parseDesktopExchangeResult(response.url);
}

/**
 * The 402 quota-refusal payload creation calls answer over the free
 * cap: the quota-exceeded error with the free-limit code, the cap and
 * its use, and the upgrade address (absent when the payload carries
 * none — the paywall falls back to the pricing page then). Refusals
 * without exactly this shape never prompt.
 */
export const QuotaExceededPayload = z.object({
  error: z.literal("quota_exceeded"),
  code: z.literal("free_issue_creation_limit_exceeded"),
  limit: z.number(),
  used: z.number(),
  upgrade_url: z.optional(z.string()),
});
export type QuotaExceededPayload = z.infer<typeof QuotaExceededPayload>;

/**
 * Read a quota refusal off a failed creation call: the 402's parsed
 * body when it matches the quota shape, else null. The paywall
 * interceptor calls this on every creation failure.
 */
export function quotaRefusalFrom(error: unknown): QuotaExceededPayload | null {
  if (!isApiError(error) || error.status !== 402) return null;
  const parsed = QuotaExceededPayload.safeParse(error.body);
  return parsed.success ? parsed.data : null;
}
