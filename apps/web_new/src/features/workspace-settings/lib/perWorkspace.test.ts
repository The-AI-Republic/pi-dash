// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Per-workspace slice of the workspace-settings catalog (WSET-001/002;
// main#675): the entries a role may open in one named workspace, and the
// highlight rules the nested rows follow.
import { describe, expect, it } from "vitest";

import { ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER, selectPermissions } from "../../../core/session/index.js";
import { isWorkspaceSettingLinkActive, permittedWorkspaceSettingGroups } from "./perWorkspace.js";

const ADMIN = selectPermissions(ROLE_ADMIN);
const MEMBER = selectPermissions(ROLE_MEMBER);
const GUEST = selectPermissions(ROLE_GUEST);

describe("permittedWorkspaceSettingGroups", () => {
  it("gives admins every entry with addresses inside the named workspace", () => {
    const groups = permittedWorkspaceSettingGroups("globex", ADMIN);
    expect(groups.map((group) => group.key)).toEqual(["administration", "developer"]);
    expect(groups[0]?.links.map((link) => link.key)).toEqual(["general", "members", "billing", "exports"]);
    expect(groups[1]?.links.map((link) => link.key)).toEqual(["webhooks", "integrations"]);
    expect(groups[0]?.links.map((link) => link.href)).toEqual([
      "/globex/settings/",
      "/globex/settings/members/",
      "/globex/settings/billing/",
      "/globex/settings/exports/",
    ]);
    expect(groups[0]?.links[1]?.label).toBe("Members");
  });

  it("drops admin-only entries and whole groups for members", () => {
    const groups = permittedWorkspaceSettingGroups("acme", MEMBER);
    expect(groups.map((group) => group.key)).toEqual(["administration"]);
    expect(groups[0]?.links.map((link) => link.key)).toEqual(["general", "members", "exports"]);
  });

  it("answers no groups for a guest, so the row has nothing to expand", () => {
    expect(permittedWorkspaceSettingGroups("acme", GUEST)).toEqual([]);
  });
});

describe("isWorkspaceSettingLinkActive", () => {
  const general = { key: "general" as const, href: "/acme/settings/" };
  const webhooks = { key: "webhooks" as const, href: "/acme/settings/webhooks/" };

  it("matches General only at the settings root", () => {
    expect(isWorkspaceSettingLinkActive("/acme/settings", "acme", general)).toBe(true);
    expect(isWorkspaceSettingLinkActive("/acme/settings/", "acme", general)).toBe(true);
    expect(isWorkspaceSettingLinkActive("/acme/settings/members/", "acme", general)).toBe(false);
  });

  it("keeps nested pages under their parent entry", () => {
    expect(isWorkspaceSettingLinkActive("/acme/settings/webhooks/", "acme", webhooks)).toBe(true);
    expect(isWorkspaceSettingLinkActive("/acme/settings/webhooks/w-1", "acme", webhooks)).toBe(true);
    expect(isWorkspaceSettingLinkActive("/acme/settings/webhooks-archive/", "acme", webhooks)).toBe(false);
    expect(isWorkspaceSettingLinkActive("/acme/settings", "acme", webhooks)).toBe(false);
  });

  it("never lights an entry from another workspace or from account settings", () => {
    expect(isWorkspaceSettingLinkActive("/globex/settings/webhooks/w-1", "acme", webhooks)).toBe(false);
    expect(isWorkspaceSettingLinkActive("/acme/settings/account/security/", "acme", general)).toBe(false);
    expect(isWorkspaceSettingLinkActive("/acme/settings/account/general/", "acme", webhooks)).toBe(false);
    expect(isWorkspaceSettingLinkActive("/acme/projects", "acme", general)).toBe(false);
  });
});
