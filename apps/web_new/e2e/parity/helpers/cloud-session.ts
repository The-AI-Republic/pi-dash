// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud-edition session cookies (NEWFRONT-984). The cloud /login is
// OIDC-only, so cloud specs cannot sign in through the UI against the
// password-seeded stack. They sign in over the API instead
// (`signInSession`, the same native form the card submits) and hand the
// session to the browser through `context.addCookies` before navigating.
import type { BrowserCookie } from "./api.js";

/** Hostname of the web_new target the `new` project drives. */
export function newUrlHostFromEnv(env: Record<string, string | undefined> = process.env): string {
  const raw = env["PARITY_NEW_URL"] ?? "http://localhost:3010";
  try {
    return new URL(raw).hostname;
  } catch {
    return "localhost";
  }
}

/**
 * Browser cookies for a session header (`signInSession` answer) so the
 * `new` target acts as that user with no UI login. Only the session and
 * CSRF cookies travel; anything else in the header is ignored.
 */
export function browserCookiesFromHeader(cookieHeader: string, domain: string = newUrlHostFromEnv()): BrowserCookie[] {
  const jar = new Map<string, string>();
  for (const pair of cookieHeader.split(";")) {
    const trimmed = pair.trim();
    const eq = trimmed.indexOf("=");
    if (eq > 0) jar.set(trimmed.slice(0, eq), trimmed.slice(eq + 1));
  }
  const out: BrowserCookie[] = [];
  for (const name of ["session-id", "csrftoken"]) {
    const value = jar.get(name);
    if (value !== undefined && value !== "")
      out.push({ name, value, domain, path: "/", httpOnly: true, sameSite: "Lax" });
  }
  return out;
}
