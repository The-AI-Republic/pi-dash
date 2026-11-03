// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud session cookies (NEWFRONT-984): the session header becomes the
// two browser cookies the `new` target needs, scoped to its own host.
import { describe, expect, it } from "vitest";

import { browserCookiesFromHeader, newUrlHostFromEnv } from "./cloud-session.js";

describe("newUrlHostFromEnv", () => {
  it("reads the new target's hostname", () => {
    expect(newUrlHostFromEnv({ PARITY_NEW_URL: "http://127.0.0.1:3010" })).toBe("127.0.0.1");
  });

  it("defaults to localhost", () => {
    expect(newUrlHostFromEnv({})).toBe("localhost");
  });

  it("falls back to localhost for a malformed URL", () => {
    expect(newUrlHostFromEnv({ PARITY_NEW_URL: "://bad" })).toBe("localhost");
  });
});

describe("browserCookiesFromHeader", () => {
  it("shapes the session and CSRF cookies for the context", () => {
    expect(browserCookiesFromHeader("csrftoken=abc; session-id=xyz", "localhost")).toEqual([
      { name: "session-id", value: "xyz", domain: "localhost", path: "/", httpOnly: true, sameSite: "Lax" },
      { name: "csrftoken", value: "abc", domain: "localhost", path: "/", httpOnly: true, sameSite: "Lax" },
    ]);
  });

  it("ignores pairs without a value and unknown cookies", () => {
    expect(browserCookiesFromHeader("session-id=; bareword; other=1; session-id=ok", "example.test")).toEqual([
      { name: "session-id", value: "ok", domain: "example.test", path: "/", httpOnly: true, sameSite: "Lax" },
    ]);
  });

  it("returns nothing without a session", () => {
    expect(browserCookiesFromHeader("", "localhost")).toEqual([]);
  });

  it("defaults the domain to the new target", () => {
    const cookies = browserCookiesFromHeader("session-id=xyz");
    expect(cookies).toHaveLength(1);
    expect(cookies[0]!.domain).toBe("localhost");
  });
});
