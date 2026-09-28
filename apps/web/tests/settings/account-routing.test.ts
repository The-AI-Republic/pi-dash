/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { describe, expect, it } from "vitest";

import { getAccountActivePath } from "@/components/settings/helper";
import { tabFromSplat } from "@/app/routes/redirects/core/settings";

// Account (profile) settings moved from the global `/settings/profile/:tab` to
// `/:workspaceSlug/settings/account/:tab` (PDASHOSS01-255). Every legacy URL
// funnels through the `/settings/*` splat, so the splat -> tab mapping is the
// contract that keeps old links alive.
describe("tabFromSplat", () => {
  it("maps the legacy profile URLs onto their tab", () => {
    expect(tabFromSplat("profile/security")).toBe("security");
    expect(tabFromSplat("profile/api-tokens")).toBe("api-tokens");
  });

  it("falls back to the general tab when no tab is named", () => {
    expect(tabFromSplat("")).toBe("general");
    expect(tabFromSplat("profile")).toBe("general");
    expect(tabFromSplat("profile/")).toBe("general");
  });

  it("keeps a bare tab that was not prefixed with `profile`", () => {
    expect(tabFromSplat("notifications")).toBe("notifications");
  });

  it("ignores anything past the tab segment", () => {
    expect(tabFromSplat("profile/activity/extra")).toBe("activity");
  });
});

describe("getAccountActivePath", () => {
  it("resolves the label the mobile nav shows for a tab", () => {
    expect(getAccountActivePath("/my-workspace/settings/account/general/")).toBe("Profile");
    expect(getAccountActivePath("/my-workspace/settings/account/api-tokens/")).toBe("Personal Access Tokens");
  });

  it("returns null outside account settings", () => {
    expect(getAccountActivePath("/my-workspace/settings/members/")).toBeNull();
    expect(getAccountActivePath("/my-workspace/")).toBeNull();
  });

  it("returns null for a bare or unknown account path", () => {
    expect(getAccountActivePath("/my-workspace/settings/account/")).toBeNull();
    expect(getAccountActivePath("/my-workspace/settings/account/nope/")).toBeUndefined();
  });
});
