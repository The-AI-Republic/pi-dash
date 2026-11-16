// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The modules list's traffic rules (MOD-042/043): module-owned requests must
// stay on the spec's endpoints, and no request at all may name a desktop
// shell, a license or a plan.
import { describe, expect, it } from "vitest";

import { entryTrafficViolations, isDesktopOrEditionPath, isModuleEndpointPath, namesModuleTraffic } from "./entryTraffic";

const LIST = "/api/workspaces/acme/projects/p1/modules/";
const DETAIL = "/api/workspaces/acme/projects/p1/modules/323e4567-e89b-12d3-a456-426614174002/";

describe("namesModuleTraffic", () => {
  it("recognizes module endpoints and invented module paths", () => {
    for (const path of [LIST, DETAIL, "/api/workspaces/acme/projects/p1/archived-modules/", "/x/module-links/"]) {
      expect(namesModuleTraffic(path), path).toBe(true);
    }
    for (const path of ["/api/workspaces/acme/projects/p1/", "/api/workspaces/acme/states/"]) {
      expect(namesModuleTraffic(path), path).toBe(false);
    }
  });
});

describe("isModuleEndpointPath", () => {
  it("accepts every endpoint the spec names", () => {
    for (const path of [
      LIST,
      DETAIL,
      "/api/workspaces/acme/projects/p1/archived-modules/",
      "/api/workspaces/acme/projects/p1/modules/1/issues/",
      "/api/workspaces/acme/projects/p1/modules/1/module-links/",
      "/api/workspaces/acme/projects/p1/user-favorite-modules/",
    ]) {
      expect(isModuleEndpointPath(path), path).toBe(true);
    }
  });

  it("rejects a module-naming path no slice owns", () => {
    expect(isModuleEndpointPath("/api/workspaces/acme/modules/")).toBe(true);
    expect(isModuleEndpointPath("/api/workspaces/acme/projects/p1/module-extras/")).toBe(false);
  });
});

describe("isDesktopOrEditionPath", () => {
  it("names the build, license and plan surfaces", () => {
    for (const path of [
      "/api/billing/subscriptions/",
      "/api/instances/licenses/",
      "/api/workspaces/acme/premium/",
      "/api/workspaces/acme/projects/p1/upgrade/",
      "/api/desktop/sync/",
      "/api/tauri/updates/",
      "/api/workspaces/acme/projects/p1/plans/",
      "/api/checkout/",
    ]) {
      expect(isDesktopOrEditionPath(path), path).toBe(true);
    }
  });

  it("leaves ordinary module and frame traffic alone", () => {
    for (const path of [
      LIST,
      DETAIL,
      "/api/workspaces/acme/projects/p1/",
      "/api/workspaces/acme/members/",
      "/api/workspaces/acme/projects/p1/estimates/",
    ]) {
      expect(isDesktopOrEditionPath(path), path).toBe(false);
    }
  });
});

describe("entryTrafficViolations", () => {
  it("passes a clean entry with module and frame traffic", () => {
    expect(
      entryTrafficViolations([
        LIST,
        DETAIL,
        "/api/workspaces/acme/projects/p1/",
        "/api/workspaces/acme/members/",
        "/api/workspaces/acme/users/notifications/unread/",
      ])
    ).toEqual({ prohibited: [], unknownModule: [] });
  });

  it("reports every prohibited path, not just the first", () => {
    const violations = entryTrafficViolations([LIST, "/api/billing/subscriptions/", "/api/desktop/sync/"]);
    expect(violations.prohibited).toEqual(["/api/billing/subscriptions/", "/api/desktop/sync/"]);
    expect(violations.unknownModule).toEqual([]);
  });

  it("reports a module path outside the spec's endpoints", () => {
    expect(entryTrafficViolations([LIST, "/api/workspaces/acme/projects/p1/module-extras/"])).toEqual({
      prohibited: [],
      unknownModule: ["/api/workspaces/acme/projects/p1/module-extras/"],
    });
  });
});
