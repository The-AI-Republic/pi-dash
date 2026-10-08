// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract-test setup. These suites only run against a live local Django
// seeded with contract/seed-contracts.py; without PIDASH_CONTRACT_BASE_URL
// every suite skips so CI stays green.
import { ApiClient, createClient } from "../client.js";
import type { Transport } from "../client.js";

export const CONTRACT_BASE_URL = process.env["PIDASH_CONTRACT_BASE_URL"] ?? "";
export const CONTRACT_EMAIL = process.env["PIDASH_CONTRACT_EMAIL"] ?? "contract.tester@example.com";
export const CONTRACT_PASSWORD = process.env["PIDASH_CONTRACT_PASSWORD"] ?? "Contract123!";
export const CONTRACT_WORKSPACE = process.env["PIDASH_CONTRACT_WORKSPACE"] ?? "contract-acme";
export const CONTRACT_PROJECT_IDENTIFIER = "CT";

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
