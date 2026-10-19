// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  APPS_CATALOG_ROUTE_PATH,
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
} from "./routes.js";
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
