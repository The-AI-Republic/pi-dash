/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { describe, expect, it, vi } from "vitest";

import { EUserPermissionsLevel, WORKSPACE_SETTINGS } from "@pi-dash/constants";
import { EUserWorkspaceRoles } from "@pi-dash/types";

import {
  getAccessibleWorkspaceSettingGroups,
  isWorkspaceSettingItemActive,
} from "@/components/settings/sidebar/workspace-setting-groups";

// The account settings navigator lists every workspace the user belongs to and expands each
// one in place (PDASHOSS01-256), so the item list has to be filtered against *that* row's
// workspace rather than the current one. These cover the two rules that governs.

/** Stands in for the permission store: grants whatever role the given workspace maps to. */
const allowPermissionsFor = (rolesBySlug: Record<string, EUserWorkspaceRoles>) =>
  vi.fn((access: (number | EUserWorkspaceRoles)[], _level: unknown, workspaceSlug?: string) => {
    const role = workspaceSlug ? rolesBySlug[workspaceSlug] : undefined;
    return role !== undefined && access.includes(role);
  });

describe("getAccessibleWorkspaceSettingGroups", () => {
  it("gives an admin every item, grouped by category", () => {
    const groups = getAccessibleWorkspaceSettingGroups("acme", allowPermissionsFor({ acme: EUserWorkspaceRoles.ADMIN }));

    expect(groups.map((group) => group.category)).toEqual(["administration", "developer"]);
    expect(groups[0].items.map((item) => item.key)).toEqual(["general", "members", "billing-and-plans", "export"]);
    expect(groups[1].items.map((item) => item.key)).toEqual(["webhooks", "integrations"]);
  });

  it("drops the items a member may not open, and any category left empty", () => {
    const groups = getAccessibleWorkspaceSettingGroups(
      "acme",
      allowPermissionsFor({ acme: EUserWorkspaceRoles.MEMBER })
    );

    // Billing is admin-only, and Developer holds nothing else a member may open.
    expect(groups.map((group) => group.category)).toEqual(["administration"]);
    expect(groups[0].items.map((item) => item.key)).toEqual(["general", "members", "export"]);
  });

  it("returns nothing for a workspace where the user may open nothing, so the row renders plain", () => {
    expect(getAccessibleWorkspaceSettingGroups("acme", allowPermissionsFor({ acme: EUserWorkspaceRoles.GUEST }))).toEqual(
      []
    );
  });

  it("filters against the workspace it was asked about, not the current one", () => {
    const allowPermissions = allowPermissionsFor({
      acme: EUserWorkspaceRoles.ADMIN,
      other: EUserWorkspaceRoles.MEMBER,
    });

    const otherGroups = getAccessibleWorkspaceSettingGroups("other", allowPermissions);

    expect(otherGroups.flatMap((group) => group.items).map((item) => item.key)).not.toContain("billing-and-plans");
    expect(allowPermissions).toHaveBeenCalledWith(expect.anything(), EUserPermissionsLevel.WORKSPACE, "other");
  });
});

describe("isWorkspaceSettingItemActive", () => {
  it("matches `general` exactly so it does not light up on every other tab", () => {
    const general = WORKSPACE_SETTINGS["general"];

    expect(isWorkspaceSettingItemActive("/acme/settings/", "acme", general)).toBe(true);
    expect(isWorkspaceSettingItemActive("/acme/settings/members/", "acme", general)).toBe(false);
  });

  it("matches the other items on prefix so nested pages keep the parent highlighted", () => {
    const members = WORKSPACE_SETTINGS["members"];

    expect(isWorkspaceSettingItemActive("/acme/settings/members/", "acme", members)).toBe(true);
    expect(isWorkspaceSettingItemActive("/acme/settings/members/invite/", "acme", members)).toBe(true);
  });

  it("only highlights the workspace actually being viewed", () => {
    const members = WORKSPACE_SETTINGS["members"];

    expect(isWorkspaceSettingItemActive("/acme/settings/members/", "other", members)).toBe(false);
  });

  it("does not highlight a workspace item while account settings are open", () => {
    const general = WORKSPACE_SETTINGS["general"];

    expect(isWorkspaceSettingItemActive("/acme/settings/account/general/", "acme", general)).toBe(false);
  });
});
