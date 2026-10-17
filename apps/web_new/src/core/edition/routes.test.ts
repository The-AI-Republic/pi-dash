// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { APPS_CATALOG_ROUTE_PATH, isEditionRouteEnabled, resolveEditionSidebarHref } from "./routes.js";
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

  it("resolves sidebar targets against the workspace slug", () => {
    expect(resolveEditionSidebarHref("acme", "/apps/")).toBe("/acme/apps/");
  });

  it("forgives a missing leading slash", () => {
    expect(resolveEditionSidebarHref("acme", "apps/")).toBe("/acme/apps/");
  });
});
