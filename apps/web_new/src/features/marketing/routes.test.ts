// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public route decisions (CLOUD-001/002), shared by the root, /login and
// their tests. Pure: no router and no edition module needed.
import { describe, expect, it } from "vitest";

import type { Edition } from "../../core/edition/index.js";
import { safeNextPath } from "../../core/session/index.js";
import {
  CLOUD_LOGIN_PATH,
  MARKETING_HOME_PATH,
  isMarketingRouteEnabled,
  ownsPublicRoot,
  signedOutRootTarget,
} from "./routes.js";

const cloud: Edition = {
  id: "cloud",
  flags: {},
  routes: [
    { path: "/", scope: "top-level" },
    { path: "home", scope: "top-level" },
    { path: "docs", scope: "top-level" },
    { path: "downloads", scope: "top-level" },
    { path: "pricing", scope: "top-level" },
    { path: "login", scope: "top-level" },
  ],
};

describe("signedOutRootTarget", () => {
  it("lands a visitor with no target on the marketing home", () => {
    expect(signedOutRootTarget(undefined)).toBe(MARKETING_HOME_PATH);
    expect(MARKETING_HOME_PATH).toBe("/home");
  });

  it("carries a safe target to the login page as next_path", () => {
    expect(signedOutRootTarget("/acme/projects/p-1?tab=issues")).toBe(
      "/login?next_path=%2Facme%2Fprojects%2Fp-1%3Ftab%3Dissues"
    );
    expect(CLOUD_LOGIN_PATH).toBe("/login");
  });

  it("never forwards an unsafe value (CLOUD-013)", () => {
    // The root passes the parser's sanitized value; an unsafe one becomes
    // absent, so the visitor simply lands on the marketing home.
    expect(signedOutRootTarget(safeNextPath("https://evil.test/phish"))).toBe("/home");
    expect(signedOutRootTarget(safeNextPath("//evil.test"))).toBe("/home");
    expect(signedOutRootTarget(safeNextPath("\\evil.test"))).toBe("/home");
  });
});

describe("edition-owned marketing surface", () => {
  it("enables the declared marketing pages on the cloud edition", () => {
    expect(isMarketingRouteEnabled(cloud, "/home")).toBe(true);
    expect(isMarketingRouteEnabled(cloud, "/docs")).toBe(true);
    expect(isMarketingRouteEnabled(cloud, "/downloads")).toBe(true);
    expect(isMarketingRouteEnabled(cloud, "/pricing")).toBe(true);
    expect(isMarketingRouteEnabled(cloud, "/home/")).toBe(true);
    expect(ownsPublicRoot(cloud)).toBe(true);
  });

  it("leaves the OSS build without a marketing surface or root forward", () => {
    const oss: Edition = { id: "oss", flags: {} };
    for (const path of ["/home", "/docs", "/downloads", "/pricing"]) {
      expect(isMarketingRouteEnabled(oss, path)).toBe(false);
    }
    expect(ownsPublicRoot(oss)).toBe(false);
  });

  it("does not enable undeclared or non-marketing paths", () => {
    const partial: Edition = { id: "cloud", flags: {}, routes: [{ path: "home", scope: "top-level" }] };
    expect(isMarketingRouteEnabled(partial, "/home")).toBe(true);
    expect(isMarketingRouteEnabled(partial, "/docs")).toBe(false);
    expect(isMarketingRouteEnabled(partial, "/somewhere")).toBe(false);
  });
});
