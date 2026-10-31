// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  APPS_CATALOG_ROUTE_PATH,
  CLOUD_LOGIN_PATH,
  MARKETING_HOME_PATH,
  PUBLIC_MARKETING_PATHS,
  ROOT_ROUTE_PATH,
  SITEMAP_MARKETING_PATHS,
  findEditionSettingsSection,
  isEditionMarketingPath,
  isEditionProviderFreePath,
  isEditionPublicRouteEnabled,
  isEditionRootEnabled,
  isEditionRouteEnabled,
  isEditionSettingsSectionEnabled,
  isTopLevelRoute,
  publicMarketingPathOf,
  resolveEditionSidebarHref,
  signedOutRootTarget,
} from "./routes.js";
import { safeNextPath } from "../session/index.js";
import type { Edition } from "./types.js";

describe("edition routes", () => {
  it("names the apps catalog segment", () => {
    expect(APPS_CATALOG_ROUTE_PATH).toBe("apps");
  });

  it("enables a route only when the edition declares its segment", () => {
    const cloud: Edition = { id: "cloud", flags: {}, routes: [{ path: "apps" }] };
    expect(isEditionRouteEnabled(cloud, "apps")).toBe(true);
    expect(isEditionRouteEnabled(cloud, "billing")).toBe(false);
  });

  it("disables every route without a declaration", () => {
    expect(isEditionRouteEnabled({ id: "oss", flags: {} }, "apps")).toBe(false);
    expect(isEditionRouteEnabled({ id: "oss", flags: {}, routes: [] }, "apps")).toBe(false);
  });

  it("keeps workspace routes distinct from top-level ones", () => {
    const cloud: Edition = {
      id: "cloud",
      flags: {},
      routes: [{ path: "apps" }, { path: "home", scope: "top-level" }],
    };
    expect(isEditionRouteEnabled(cloud, "apps")).toBe(true);
    expect(isEditionRouteEnabled(cloud, "home")).toBe(false);
    expect(isEditionPublicRouteEnabled(cloud, "home")).toBe(true);
    expect(isEditionPublicRouteEnabled(cloud, "/home/")).toBe(true);
    expect(isEditionPublicRouteEnabled(cloud, "apps")).toBe(false);
    expect(isTopLevelRoute({ path: "home", scope: "top-level" })).toBe(true);
    expect(isTopLevelRoute({ path: "apps" })).toBe(false);
  });

  it("declares the public addresses the cloud module must opt into", () => {
    const cloud: Edition = {
      id: "cloud",
      flags: {},
      routes: [
        { path: "/", scope: "top-level" },
        { path: "home", scope: "top-level" },
        { path: "docs", scope: "top-level" },
        { path: "downloads", scope: "top-level" },
        { path: "pricing", scope: "top-level" },
      ],
    };
    expect(PUBLIC_MARKETING_PATHS).toEqual(["/home", "/docs", "/downloads", "/pricing"]);
    expect(SITEMAP_MARKETING_PATHS).toEqual(["/home", "/docs", "/pricing"]);
    for (const path of PUBLIC_MARKETING_PATHS) {
      expect(isEditionMarketingPath(cloud, path)).toBe(true);
      expect(isEditionProviderFreePath(cloud, path)).toBe(true);
      expect(publicMarketingPathOf(path)).toBe(path);
    }
    expect(isEditionRootEnabled(cloud)).toBe(true);
    expect(isEditionPublicRouteEnabled(cloud, "/")).toBe(true);
  });

  it("keeps the OSS edition out of every public marketing address", () => {
    const oss: Edition = { id: "oss", flags: {}, routes: [{ path: "apps" }] };
    for (const path of PUBLIC_MARKETING_PATHS) {
      expect(isEditionMarketingPath(oss, path)).toBe(false);
      expect(isEditionProviderFreePath(oss, path)).toBe(false);
    }
    expect(isEditionRootEnabled(oss)).toBe(false);
    expect(isEditionMarketingPath(oss, "/not-a-page")).toBe(false);
    expect(publicMarketingPathOf("/not-a-page")).toBe(null);
    expect(publicMarketingPathOf("/home/")).toBe("/home");
    expect(ROOT_ROUTE_PATH).toBe("/");
  });

  it("does not treat a trailing-slash-free spelling as a different address", () => {
    expect(publicMarketingPathOf("home")).toBe("/home");
    expect(publicMarketingPathOf("///")).toBe(null);
  });

  it("resolves sidebar targets against the workspace slug", () => {
    expect(resolveEditionSidebarHref("acme", "/apps/")).toBe("/acme/apps/");
  });

  it("forgives a missing leading slash", () => {
    expect(resolveEditionSidebarHref("acme", "apps/")).toBe("/acme/apps/");
  });
});

describe("edition settings sections", () => {
  const cloud: Edition = {
    id: "cloud",
    flags: {},
    settingsSections: [{ id: "ai-assistant", label: "AI assistant" }],
  };

  it("enables only the sections the edition declares", () => {
    expect(isEditionSettingsSectionEnabled(cloud, "ai-assistant")).toBe(true);
    expect(isEditionSettingsSectionEnabled(cloud, "security")).toBe(false);
  });

  it("enables nothing when the edition declares no sections", () => {
    expect(isEditionSettingsSectionEnabled({ id: "oss", flags: {} }, "ai-assistant")).toBe(false);
    expect(isEditionSettingsSectionEnabled({ id: "oss", flags: {}, settingsSections: [] }, "ai-assistant")).toBe(false);
  });

  it("returns the declared section so its label can be shown", () => {
    expect(findEditionSettingsSection(cloud, "ai-assistant")).toEqual({ id: "ai-assistant", label: "AI assistant" });
  });

  it("returns null for an undeclared section", () => {
    expect(findEditionSettingsSection(cloud, "security")).toBeNull();
    expect(findEditionSettingsSection({ id: "oss", flags: {} }, "ai-assistant")).toBeNull();
  });
});

describe("signedOutRootTarget (CLOUD-001)", () => {
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

  it("enables the declared marketing pages on the cloud edition", () => {
    for (const path of ["/home", "/docs", "/downloads", "/pricing", "/home/"]) {
      expect(isEditionMarketingPath(cloud, path)).toBe(true);
    }
    expect(isEditionRootEnabled(cloud)).toBe(true);
  });

  it("leaves the OSS build without a marketing surface or root forward", () => {
    const oss: Edition = { id: "oss", flags: {} };
    for (const path of ["/home", "/docs", "/downloads", "/pricing"]) {
      expect(isEditionMarketingPath(oss, path)).toBe(false);
    }
    expect(isEditionRootEnabled(oss)).toBe(false);
  });

  it("does not enable undeclared or non-marketing paths", () => {
    const partial: Edition = { id: "cloud", flags: {}, routes: [{ path: "home", scope: "top-level" }] };
    expect(isEditionMarketingPath(partial, "/home")).toBe(true);
    expect(isEditionMarketingPath(partial, "/docs")).toBe(false);
    expect(isEditionMarketingPath(partial, "/somewhere")).toBe(false);
  });
});
