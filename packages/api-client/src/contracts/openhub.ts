// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// OpenHub catalog contracts: the cloud Apps catalog behind the workspace
// Apps page (NEWFRONT-641, rows CLOUD-022-031). Pi Dash proxies the Hub —
// the browser never sees the platform credential — so Hub-defined answer
// bodies are captured as the fields the UI reads, with anything else
// stripped. Django-side shapes (status, request validation) come from the
// cloud backend's openhub views; Hub payload keys follow the shapes the
// cloud oracle proved (marketplace pages, app rows, auth states).
import * as z from "zod/mini";
import { ApiClient } from "../client.js";

/**
 * The catalog gate. `connected` plus `has_gateway_scopes` decide which
 * Apps-page state renders; a disconnected answer carries the server's
 * machine code and detail (or neither, and the UI falls back).
 */
export const OpenhubStatus = z.object({
  connected: z.boolean(),
  has_gateway_scopes: z.boolean(),
  hub_url: z.string(),
  wallet_url: z.optional(z.string()),
  scopes: z.optional(z.array(z.string())),
  credential_type: z.optional(z.nullable(z.string())),
  error: z.optional(z.string()),
  detail: z.optional(z.string()),
});
export type OpenhubStatus = z.infer<typeof OpenhubStatus>;

export async function getOpenhubStatus(client: ApiClient): Promise<OpenhubStatus> {
  const body = await client.getJson<unknown>("/api/openhub/status/");
  return client.parse(OpenhubStatus, body, "GET /api/openhub/status/");
}

/**
 * One catalog row. Identity (`appId`, `slug`, `name`) is always present;
 * install state arrives as the installed flag or an installed status, and
 * the unfinished marker comes from the service's suggested action — never
 * derived locally. Icon sources prefer the self-hosted graphic, then the
 * proxied external address.
 */
export const OpenhubApp = z.object({
  appId: z.string(),
  slug: z.string(),
  name: z.string(),
  description: z.optional(z.string()),
  authType: z.optional(z.string()),
  isInstalled: z.optional(z.boolean()),
  status: z.optional(z.string()),
  suggestedAction: z.optional(z.string()),
  iconSvgFallback: z.optional(z.string()),
  iconUrl: z.optional(z.string()),
});
export type OpenhubApp = z.infer<typeof OpenhubApp>;

/** One catalog window: items plus the cursor for the next window. */
export const OpenhubMarketplacePage = z.object({
  items: z.array(OpenhubApp),
  nextCursor: z.optional(z.nullable(z.string())),
  totalCount: z.optional(z.number()),
  userStateAvailable: z.optional(z.boolean()),
});
export type OpenhubMarketplacePage = z.infer<typeof OpenhubMarketplacePage>;

export interface MarketplaceQuery {
  q?: string;
  cursor?: string;
  limit?: number;
  resourceKind?: string;
}

export async function listMarketplaceApps(
  client: ApiClient,
  query: MarketplaceQuery = {}
): Promise<OpenhubMarketplacePage> {
  const body = await client.getJson<unknown>("/api/openhub/apps/marketplace/", {
    query: { q: query.q, cursor: query.cursor, limit: query.limit, resourceKind: query.resourceKind },
  });
  return client.parse(OpenhubMarketplacePage, body, "GET /api/openhub/apps/marketplace/");
}

/**
 * The caller's own installed apps. Personal by design — an installation
 * belongs to the account, not the workspace the page was opened from.
 */
export async function listInstalledApps(client: ApiClient): Promise<OpenhubMarketplacePage> {
  const body = await client.getJson<unknown>("/api/openhub/apps/installations/");
  return client.parse(OpenhubMarketplacePage, body, "GET /api/openhub/apps/installations/");
}

/**
 * One app's Hub manifest. Hub-defined and opaque to this UI (no surface
 * reads it); parsed as an object so a non-JSON answer still fails loudly.
 */
export const OpenhubAppManifest = z.record(z.string(), z.unknown());
export type OpenhubAppManifest = z.infer<typeof OpenhubAppManifest>;

export async function getAppManifest(client: ApiClient, appId: string): Promise<OpenhubAppManifest> {
  const body = await client.getJson<unknown>(`/api/openhub/apps/${encodeURIComponent(appId)}/manifest/`);
  return client.parse(OpenhubAppManifest, body, `GET /api/openhub/apps/${appId}/manifest/`);
}

/**
 * The install/remove answer. Hub-defined; the observed bodies echo the
 * installed flag, and anything else strips away — success is the 2xx.
 */
export const OpenhubInstallResult = z.object({
  installed: z.optional(z.boolean()),
});
export type OpenhubInstallResult = z.infer<typeof OpenhubInstallResult>;

export async function installApp(client: ApiClient, appId: string): Promise<OpenhubInstallResult> {
  const envelope = await client.sendJson<unknown>("POST", `/api/openhub/apps/${encodeURIComponent(appId)}/install/`, {
    json: {},
  });
  return client.parse(OpenhubInstallResult, envelope.data, `POST /api/openhub/apps/${appId}/install/`);
}

export async function uninstallApp(client: ApiClient, appId: string): Promise<OpenhubInstallResult> {
  const envelope = await client.sendJson<unknown>("DELETE", `/api/openhub/apps/${encodeURIComponent(appId)}/install/`);
  return client.parse(OpenhubInstallResult, envelope.data, `DELETE /api/openhub/apps/${appId}/install/`);
}

/**
 * The activation-toggle answer. Hub-defined and opaque (no control in the
 * UI reads it); parsed as an object so a non-JSON answer fails loudly.
 */
export const OpenhubActivationResult = z.record(z.string(), z.unknown());
export type OpenhubActivationResult = z.infer<typeof OpenhubActivationResult>;

export async function setAppActivation(
  client: ApiClient,
  appId: string,
  activate: boolean
): Promise<OpenhubActivationResult> {
  const envelope = await client.sendJson<unknown>(
    "POST",
    `/api/openhub/apps/${encodeURIComponent(appId)}/activation/`,
    { json: { activate } }
  );
  return client.parse(OpenhubActivationResult, envelope.data, `POST /api/openhub/apps/${appId}/activation/`);
}

/** One credential field of the key-setup manifest. */
export const OpenhubCredentialField = z.object({
  key: z.string(),
  label: z.string(),
  type: z.string(),
  placeholder: z.optional(z.string()),
  validation: z.optional(z.string()),
});
export type OpenhubCredentialField = z.infer<typeof OpenhubCredentialField>;

/** The key-setup manifest behind the generic credential dialog. */
export const OpenhubManualSetup = z.object({
  setupUrl: z.optional(z.string()),
  instructions_md: z.optional(z.string()),
  fields: z.optional(z.array(OpenhubCredentialField)),
});
export type OpenhubManualSetup = z.infer<typeof OpenhubManualSetup>;

/**
 * Per-app connection state. Key-based apps carry the setup manifest; a
 * bare key type with no manifest falls back to a single secret field.
 */
export const OpenhubAuthStatus = z.object({
  type: z.string(),
  status: z.string(),
  manualSetup: z.optional(OpenhubManualSetup),
});
export type OpenhubAuthStatus = z.infer<typeof OpenhubAuthStatus>;

export async function getAppAuthStatus(client: ApiClient, appId: string): Promise<OpenhubAuthStatus> {
  const body = await client.getJson<unknown>(`/api/openhub/apps/${encodeURIComponent(appId)}/auth/status/`);
  return client.parse(OpenhubAuthStatus, body, `GET /api/openhub/apps/${appId}/auth/status/`);
}

/**
 * The OAuth-start answer: the provider authorize URL the handoff tab
 * navigates to. The server validates it is a plain credentialless HTTPS
 * address before answering, so it is always present on success.
 */
export const OpenhubOAuthStart = z.object({
  authorizationUrl: z.string(),
});
export type OpenhubOAuthStart = z.infer<typeof OpenhubOAuthStart>;

export async function startAppOAuth(client: ApiClient, appId: string, returnPath?: string): Promise<OpenhubOAuthStart> {
  const envelope = await client.sendJson<unknown>(
    "POST",
    `/api/openhub/apps/${encodeURIComponent(appId)}/auth/oauth/start/`,
    { json: { return_path: returnPath } }
  );
  return client.parse(OpenhubOAuthStart, envelope.data, `POST /api/openhub/apps/${appId}/auth/oauth/start/`);
}

/**
 * The consent-handoff address. Navigated to, never fetched: the UI points
 * its synchronously pre-opened tab here and the server 302s to provider
 * consent (or back to the return path with an error when the handoff
 * cannot start).
 */
export function appOAuthBeginUrl(apiBase: string, appId: string, returnPath: string): string {
  const base = apiBase.replace(/\/+$/, "");
  return `${base}/api/openhub/apps/${encodeURIComponent(appId)}/auth/oauth/begin/?return_path=${encodeURIComponent(returnPath)}`;
}

export interface AppApiKeyInput {
  fields: Record<string, string>;
  accountHint?: string;
}

/**
 * The credential-submit answer. Hub-defined; the observed bodies echo the
 * connected flag — success is the 2xx, failures stay in the dialog with
 * the server's rejection.
 */
export const OpenhubApiKeyResult = z.object({
  connected: z.optional(z.boolean()),
});
export type OpenhubApiKeyResult = z.infer<typeof OpenhubApiKeyResult>;

export async function submitAppApiKey(
  client: ApiClient,
  appId: string,
  input: AppApiKeyInput
): Promise<OpenhubApiKeyResult> {
  const envelope = await client.sendJson<unknown>(
    "POST",
    `/api/openhub/apps/${encodeURIComponent(appId)}/auth/api-key/`,
    { json: { fields: input.fields, account_hint: input.accountHint } }
  );
  return client.parse(OpenhubApiKeyResult, envelope.data, `POST /api/openhub/apps/${appId}/auth/api-key/`);
}

/**
 * The icon-proxy address for an external icon URL. Built, never fetched
 * here — it is an image source; the server validates the shape before
 * fetching and answers 400/404 otherwise.
 */
export function appIconUrl(apiBase: string, iconUrl: string): string {
  const base = apiBase.replace(/\/+$/, "");
  return `${base}/api/openhub/apps/icon/?url=${encodeURIComponent(iconUrl)}`;
}
