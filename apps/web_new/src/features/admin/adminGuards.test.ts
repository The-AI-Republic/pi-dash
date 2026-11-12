// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { AdminSession } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { GOD_MODE_ENTRY } from "./adminErrors.js";
import { decideAdminGuard, GOD_MODE_GENERAL } from "./adminGuards.js";

const ADMIN: AdminSession = { kind: "admin", admin: { email: "admin@example.com" } as never };
const ANONYMOUS: AdminSession = { kind: "anonymous" };
const FORBIDDEN: AdminSession = { kind: "forbidden" };

describe("decideAdminGuard", () => {
  it("sends a signed-in admin from the home tree to general settings", () => {
    expect(decideAdminGuard({ tree: "home", session: ADMIN })).toEqual({
      action: "redirect",
      target: GOD_MODE_GENERAL,
    });
    expect(GOD_MODE_GENERAL).toBe("/god-mode/general/");
  });

  it("renders the home tree for anonymous, revoked, and unreadable sessions", () => {
    expect(decideAdminGuard({ tree: "home", session: ANONYMOUS })).toEqual({ action: "render" });
    expect(decideAdminGuard({ tree: "home", session: FORBIDDEN })).toEqual({ action: "render" });
    expect(decideAdminGuard({ tree: "home", session: null })).toEqual({ action: "render" });
  });

  it("renders the dashboard tree for a signed-in admin", () => {
    expect(decideAdminGuard({ tree: "dashboard", session: ADMIN })).toEqual({ action: "render" });
  });

  it("bounces anonymous, revoked, and unreadable sessions off the dashboard to the root", () => {
    expect(decideAdminGuard({ tree: "dashboard", session: ANONYMOUS })).toEqual({
      action: "redirect",
      target: GOD_MODE_ENTRY,
    });
    expect(decideAdminGuard({ tree: "dashboard", session: FORBIDDEN })).toEqual({
      action: "redirect",
      target: GOD_MODE_ENTRY,
    });
    expect(decideAdminGuard({ tree: "dashboard", session: null })).toEqual({
      action: "redirect",
      target: GOD_MODE_ENTRY,
    });
  });
});
