// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract-test setup. These suites only run against a live local Django
// seeded with contract/seed-contracts.py; without PIDASH_CONTRACT_BASE_URL
// every suite skips so CI stays green.
import { ApiClient, createClient } from "../client.js";
import type { Transport } from "../client.js";
import { signIn } from "../contracts/auth.js";

export const CONTRACT_BASE_URL = process.env["PIDASH_CONTRACT_BASE_URL"] ?? "";
export const CONTRACT_EMAIL = process.env["PIDASH_CONTRACT_EMAIL"] ?? "contract.tester@example.com";
export const CONTRACT_PASSWORD = process.env["PIDASH_CONTRACT_PASSWORD"] ?? "Contract123!";
export const CONTRACT_WORKSPACE = process.env["PIDASH_CONTRACT_WORKSPACE"] ?? "contract-acme";
export const CONTRACT_PROJECT_IDENTIFIER = "CT";
export const CONTRACT_SECONDARY_EMAIL = process.env["PIDASH_CONTRACT_SECONDARY_EMAIL"] ?? "contract.probe@example.com";
export const CONTRACT_AUTOSET_EMAIL = process.env["PIDASH_CONTRACT_AUTOSET_EMAIL"] ?? "contract.autoset@example.com";

export const contractEnabled = CONTRACT_BASE_URL.length > 0;

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

export function contractTransport(jar = new Map<string, string>()): Transport {
  return async (url, init) => {
    // Walk redirects manually: fetch hides intermediate Set-Cookie headers
    // when it follows them itself, which would drop the session cookie the
    // sign-in POST sets on its 302. A browser jar keeps those; so do we.
    let currentUrl = url;
    let method = init.method;
    let body = init.body;
    let hops = 0;
    for (;;) {
      const headers: Record<string, string> = { ...init.headers };
      if (jar.size > 0) {
        headers["Cookie"] = [...jar.entries()].map(([name, value]) => `${name}=${value}`).join("; ");
      }
      const response = await globalThis.fetch(currentUrl, {
        method,
        headers,
        ...(body === undefined ? {} : { body }),
        ...(init.signal === undefined ? {} : { signal: init.signal }),
        credentials: "include",
        redirect: "manual",
      });
      storeCookies(jar, response.headers);
      const location = response.headers.get("location");
      if (!REDIRECT_STATUSES.has(response.status) || !location || hops >= 10) return response;
      hops += 1;
      currentUrl = new URL(location, currentUrl).toString();
      if (response.status === 303 || ((response.status === 301 || response.status === 302) && method === "POST")) {
        method = "GET";
        body = undefined;
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
