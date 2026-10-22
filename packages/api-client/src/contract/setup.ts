// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract-test setup. These suites only run against a live local Django
// seeded with contract/seed-contracts.py; without PIDASH_CONTRACT_BASE_URL
// every suite skips so CI stays green.
import { ApiClient, createClient } from "../client.js";
import type { Transport } from "../client.js";
import { signIn } from "../contracts/auth-entry.js";

export const CONTRACT_BASE_URL = process.env["PIDASH_CONTRACT_BASE_URL"] ?? "";
export const CONTRACT_EMAIL = process.env["PIDASH_CONTRACT_EMAIL"] ?? "contract.tester@example.com";
export const CONTRACT_PASSWORD = process.env["PIDASH_CONTRACT_PASSWORD"] ?? "Contract123!";
export const CONTRACT_WORKSPACE = process.env["PIDASH_CONTRACT_WORKSPACE"] ?? "contract-acme";
/**
 * Developer-integration fixtures live in their own workspace (NEWFRONT-533):
 * the seed plants a connected GitHub integration and provider accounts
 * there so the shared workspace keeps the "no provider account" shape the
 * NEWFRONT-569 repository suite asserts.
 */
export const CONTRACT_DEV_WORKSPACE = process.env["PIDASH_CONTRACT_DEV_WORKSPACE"] ?? "contract-dev";
export const CONTRACT_PROJECT_IDENTIFIER = "CT";
/**
 * The published board the space suites read (NEWFRONT-752). The seed mints
 * one board on the seeded project with this fixed anchor, so the public
 * board endpoints resolve without a publish round-trip.
 */
export const CONTRACT_BOARD_ANCHOR = process.env["PIDASH_CONTRACT_BOARD_ANCHOR"] ?? "contract-board-anchor";
export const CONTRACT_SECONDARY_EMAIL = process.env["PIDASH_CONTRACT_SECONDARY_EMAIL"] ?? "contract.probe@example.com";
export const CONTRACT_AUTOSET_EMAIL = process.env["PIDASH_CONTRACT_AUTOSET_EMAIL"] ?? "contract.autoset@example.com";
export const CONTRACT_ADMIN_EMAIL = process.env["PIDASH_CONTRACT_ADMIN_EMAIL"] ?? "contract.admin@example.com";
export const CONTRACT_DESKTOP_SESSION_KEY =
  process.env["PIDASH_CONTRACT_DESKTOP_SESSION_KEY"] ?? "contractdesktopsession000001";

/**
 * Cloud-edition contract backend. The cloud endpoints (openhub catalog,
 * assistant connections, provider login) exist only in the cloud Django,
 * so their suites gate on this base URL instead of the OSS one: a
 * scratch DEBUG cloud backend without a gateway, the same shape the
 * parity cloud stack serves. Shared by the NEWFRONT-638/641 suites.
 */
export const CONTRACT_CLOUD_BASE_URL = process.env["PIDASH_CONTRACT_CLOUD_BASE_URL"] ?? "";

export const contractEnabled = CONTRACT_BASE_URL.length > 0;

export const cloudContractEnabled = CONTRACT_CLOUD_BASE_URL.length > 0;

/**
 * Transport with a minimal cookie jar. Node's fetch does not persist
 * cookies, but the session backend needs the sessionid (and the CSRF flow
 * needs csrftoken) sent back on later requests.
 */
function storeCookies(jar: Map<string, string>, headers: Headers): void {
  const withCookies = headers as Headers & { getSetCookie?: () => string[] };
  const setCookies: string[] = typeof withCookies.getSetCookie === "function" ? withCookies.getSetCookie() : [];
  for (const cookie of setCookies) {
    const pair = cookie.split(";", 1)[0] ?? "";
    const separator = pair.indexOf("=");
    if (separator > 0) jar.set(pair.slice(0, separator).trim(), pair.slice(separator + 1).trim());
  }
}

const REDIRECT_STATUSES = new Set([301, 302, 303, 307, 308]);

export interface ContractTransportOptions {
  /**
   * Walk 3xx responses to their final hop. Defaults to true, which is what
   * a browser does. The spaces auth suites turn it off: their success and
   * failure redirects both land on `/spaces/...`, a frontend path the
   * Django-only contract stack does not serve, so following it would turn a
   * correct 302 into a 404 before the suite could read the Location.
   */
  followRedirects?: boolean;
}

export function contractTransport(jar = new Map<string, string>(), options: ContractTransportOptions = {}): Transport {
  const followRedirects = options.followRedirects ?? true;
  return async (url, init) => {
    // Walk redirects manually: fetch hides intermediate Set-Cookie headers
    // when it follows them itself, which would drop the session cookie the
    // sign-in POST sets on its 302. A browser jar keeps those; so do we.
    let currentUrl = url;
    let method = init.method;
    let body = init.body;
    let formData = init.formData;
    let hops = 0;
    for (;;) {
      const headers: Record<string, string> = { ...init.headers };
      if (jar.size > 0) {
        headers["Cookie"] = [...jar.entries()].map(([name, value]) => `${name}=${value}`).join("; ");
      }
      const outgoing = formData ?? body;
      const response = await globalThis.fetch(currentUrl, {
        method,
        headers,
        ...(outgoing === undefined ? {} : { body: outgoing }),
        ...(init.signal === undefined ? {} : { signal: init.signal }),
        credentials: "include",
        redirect: "manual",
      });
      storeCookies(jar, response.headers);
      if (!followRedirects) return response;
      const location = response.headers.get("location");
      if (!REDIRECT_STATUSES.has(response.status) || !location || hops >= 10) return response;
      hops += 1;
      currentUrl = new URL(location, currentUrl).toString();
      if (response.status === 303 || ((response.status === 301 || response.status === 302) && method === "POST")) {
        method = "GET";
        body = undefined;
        formData = undefined;
      }
    }
  };
}

export function contractClient(): ApiClient {
  if (!contractEnabled) throw new Error("PIDASH_CONTRACT_BASE_URL is not set");
  return createClient({ baseUrl: CONTRACT_BASE_URL, transport: contractTransport(), validate: true });
}

let sharedClient: ApiClient | null = null;

/**
 * One signed-in session per test file. Anonymous auth endpoints (CSRF
 * token, email check, code generation) share the product's 30/minute
 * throttle, so every suite signing in per test trips 429s once the package
 * grows past a handful of files. Authenticated traffic is not throttled,
 * so suites share a session and prove their contracts against it.
 */
export async function sharedSignedIn(): Promise<ApiClient> {
  if (!sharedClient) {
    const client = contractClient();
    const result = await signIn(client, { email: CONTRACT_EMAIL, password: CONTRACT_PASSWORD });
    if (!result.ok) throw new Error(`contract sign-in failed: ${result.code} ${result.message}`);
    sharedClient = client;
  }
  return sharedClient;
}

let sharedDesktopClient: ApiClient | null = null;

/**
 * One desktop-marked session for the desktop-gated suites (NEWFRONT-585).
 * The desktop-gated endpoints refuse browser sessions, and no HTTP flow in
 * this build stamps the desktop marker, so the suites ride the fixed-key
 * session row the seed script mints instead of signing in. The jar is
 * preloaded with that session cookie (`session-id`, per
 * SESSION_COOKIE_NAME); CSRF and later cookies accumulate from there like
 * any signed-in client.
 */
export async function sharedDesktopSignedIn(): Promise<ApiClient> {
  if (!sharedDesktopClient) {
    if (!contractEnabled) throw new Error("PIDASH_CONTRACT_BASE_URL is not set");
    const jar = new Map<string, string>([["session-id", CONTRACT_DESKTOP_SESSION_KEY]]);
    const client = createClient({ baseUrl: CONTRACT_BASE_URL, transport: contractTransport(jar), validate: true });
    const me = await client.getJson<{ email?: unknown }>("/api/users/me/").catch(() => null);
    if (!me || typeof me.email !== "string") {
      throw new Error("contract desktop session is missing: re-run seed-contracts.py --apply");
    }
    sharedDesktopClient = client;
  }
  return sharedDesktopClient;
}

let sharedAdminClient: ApiClient | null = null;

/**
 * One admin-signed-in session for the god-mode suites (NEWFRONT-502/503).
 * Admin endpoints live under /api/instances/, which the session middleware
 * reads through the admin-session-id cookie — a member session (session-id)
 * authenticates as anonymous there. This signs in through the native admin
 * form POST and returns a client whose jar carries the admin grant.
 */
export async function sharedAdminSignedIn(): Promise<ApiClient> {
  if (!sharedAdminClient) {
    if (!contractEnabled) throw new Error("PIDASH_CONTRACT_BASE_URL is not set");
    const jar = new Map<string, string>();
    const transport = contractTransport(jar);
    const client = createClient({ baseUrl: CONTRACT_BASE_URL, transport, validate: true });
    // Warm the jar with a CSRF token (same endpoint the member flow uses).
    const tokenBody = await client.getJson<{ csrf_token: string }>("/auth/get-csrf-token/", { csrf: false });
    const token = tokenBody.csrf_token;
    const form = new URLSearchParams({ email: CONTRACT_EMAIL, password: CONTRACT_PASSWORD }).toString();
    // POST via the transport directly: the success redirect lands on a
    // frontend path Django answers 404, which client.post would throw on
    // before we can read the landing URL. The transport returns it.
    const response = await transport(`${CONTRACT_BASE_URL}/api/instances/admins/sign-in/`, {
      method: "POST",
      headers: {
        "Content-Type": "application/x-www-form-urlencoded",
        "X-CSRFTOKEN": token,
      },
      body: form,
      credentials: "include",
    });
    const query = response.url.includes("?") ? response.url.slice(response.url.indexOf("?") + 1) : "";
    const code = new URLSearchParams(query).get("error_code");
    if (code) throw new Error(`contract admin sign-in failed: ${code} ${response.url}`);
    if (!jar.has("admin-session-id")) throw new Error("contract admin sign-in set no admin session");
    sharedAdminClient = client;
  }
  return sharedAdminClient;
}

let sharedCloudClient: ApiClient | null = null;

/**
 * One cloud-signed-in session per process, via the DEBUG-only dev-login
 * (the OSS form sign-in does not exist on the cloud URL conf). A fresh
 * address per process keeps suites from inheriting each other's rows.
 * The cloud REST APIs are CSRF-exempt; the client still attempts the
 * token read and proceeds without one when it 404s.
 */
export async function sharedCloudSignedIn(): Promise<ApiClient> {
  if (!sharedCloudClient) {
    if (!cloudContractEnabled) throw new Error("PIDASH_CONTRACT_CLOUD_BASE_URL is not set");
    const client = createClient({
      baseUrl: CONTRACT_CLOUD_BASE_URL,
      transport: contractTransport(),
      validate: true,
    });
    const email = `contract-cloud-${Date.now()}@example.com`;
    const envelope = await client.sendJson<{ user?: { id?: unknown } }>("POST", "/api/auth/dev-login/", {
      json: { email },
      csrf: false,
    });
    const userId = envelope.data.user?.id;
    if (typeof userId !== "string" || userId === "") {
      throw new Error("contract cloud dev-login carried no user id");
    }
    sharedCloudClient = client;
  }
  return sharedCloudClient;
}
