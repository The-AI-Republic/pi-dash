// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Section catalog, addressing and role filtering (WSET-001/002/036,
// spec §1). The oracle pins the labels and their order, so the catalog
// tests assert them verbatim.
import { describe, expect, it } from "vitest";

import { ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER, selectPermissions } from "../../../core/session/index.js";
import {
  canOpenWorkspaceSettingsSection,
  findWorkspaceSettingsSection,
  isWorkspaceSettingsPath,
  resolveWorkspaceSettingsSection,
  visibleWorkspaceSettingsGroups,
  WORKSPACE_SETTINGS_GROUPS,
  workspaceSettingsHref,
  workspaceSettingsRoleLabel,
  type WorkspaceSettingsSectionKey,
} from "./sections.js";

const ADMIN = selectPermissions(ROLE_ADMIN);
const MEMBER = selectPermissions(ROLE_MEMBER);
const GUEST = selectPermissions(ROLE_GUEST);

describe("workspace settings section catalog", () => {
  it("groups Administration over Developer with the pinned labels", () => {
    expect(WORKSPACE_SETTINGS_GROUPS.map((group) => group.label)).toEqual(["Administration", "Developer"]);
    expect(WORKSPACE_SETTINGS_GROUPS[0]?.sections.map((section) => section.label)).toEqual([
      "General",
      "Members",
      "Billing & Plans",
      "Exports",
    ]);
    expect(WORKSPACE_SETTINGS_GROUPS[1]?.sections.map((section) => section.label)).toEqual([
      "Webhooks",
      "Integrations (github)",
    ]);
  });

  it("titles each section with its tab name", () => {
    const titles = WORKSPACE_SETTINGS_GROUPS.flatMap((group) => group.sections).map((section) => section.title);
    expect(titles).toEqual(["General settings", "Members", "Billing & Plans", "Exports", "Webhooks", "Integrations"]);
  });

  it("finds a section or names the unknown key", () => {
    expect(findWorkspaceSettingsSection("webhooks").suffix).toBe("webhooks");
    expect(() => findWorkspaceSettingsSection("nope" as WorkspaceSettingsSectionKey)).toThrow(/Unknown/);
  });
});

describe("workspaceSettingsHref", () => {
  it("points the general section at the settings root and others at their suffix", () => {
    expect(workspaceSettingsHref("acme", "general")).toBe("/acme/settings/");
    expect(workspaceSettingsHref("acme", "members")).toBe("/acme/settings/members/");
  });
});

describe("isWorkspaceSettingsPath", () => {
  it("accepts only the settings subtree of a workspace", () => {
    expect(isWorkspaceSettingsPath("/acme/settings")).toBe(true);
    expect(isWorkspaceSettingsPath("/acme/settings/members/")).toBe(true);
    expect(isWorkspaceSettingsPath("/acme/projects/settings")).toBe(false);
    expect(isWorkspaceSettingsPath("/acme")).toBe(false);
    expect(isWorkspaceSettingsPath("/")).toBe(false);
  });
});

describe("resolveWorkspaceSettingsSection", () => {
  it("maps the root, section and nested detail addresses", () => {
    expect(resolveWorkspaceSettingsSection("/acme/settings")).toBe("general");
    expect(resolveWorkspaceSettingsSection("/acme/settings/")).toBe("general");
    expect(resolveWorkspaceSettingsSection("/acme/settings/members/")).toBe("members");
    expect(resolveWorkspaceSettingsSection("/acme/settings/webhooks/abc")).toBe("webhooks");
  });

  it("returns null outside settings and for an unknown section", () => {
    expect(resolveWorkspaceSettingsSection("/acme/projects")).toBeNull();
    expect(resolveWorkspaceSettingsSection("/settings/members")).toBeNull();
    expect(resolveWorkspaceSettingsSection("/acme/settings/projects/p1")).toBeNull();
  });
});

describe("canOpenWorkspaceSettingsSection", () => {
  it("admits admins to every section", () => {
    for (const section of WORKSPACE_SETTINGS_GROUPS.flatMap((group) => group.sections)) {
      expect(canOpenWorkspaceSettingsSection(section.key, ADMIN)).toBe(true);
    }
  });

  it("admits members to general, members and exports only", () => {
    expect(canOpenWorkspaceSettingsSection("general", MEMBER)).toBe(true);
    expect(canOpenWorkspaceSettingsSection("members", MEMBER)).toBe(true);
    expect(canOpenWorkspaceSettingsSection("exports", MEMBER)).toBe(true);
    expect(canOpenWorkspaceSettingsSection("billing", MEMBER)).toBe(false);
    expect(canOpenWorkspaceSettingsSection("webhooks", MEMBER)).toBe(false);
    expect(canOpenWorkspaceSettingsSection("integrations", MEMBER)).toBe(false);
  });

  it("refuses guests everywhere, even where a member would pass", () => {
    for (const section of WORKSPACE_SETTINGS_GROUPS.flatMap((group) => group.sections)) {
      expect(canOpenWorkspaceSettingsSection(section.key, GUEST)).toBe(false);
    }
  });
});

describe("visibleWorkspaceSettingsGroups", () => {
  it("lists both groups for admins", () => {
    const groups = visibleWorkspaceSettingsGroups("acme", ADMIN);
    expect(groups.map((group) => group.label)).toEqual(["Administration", "Developer"]);
    expect(groups.flatMap((group) => group.sections).map((section) => section.label)).toEqual([
      "General",
      "Members",
      "Billing & Plans",
      "Exports",
      "Webhooks",
      "Integrations (github)",
    ]);
    expect(groups[1]?.sections[0]?.href).toBe("/acme/settings/webhooks/");
  });

  it("drops the Developer group entirely for members", () => {
    const groups = visibleWorkspaceSettingsGroups("acme", MEMBER);
    expect(groups.map((group) => group.label)).toEqual(["Administration"]);
    expect(groups[0]?.sections.map((section) => section.label)).toEqual(["General", "Members", "Exports"]);
  });

  it("renders nothing for guests", () => {
    expect(visibleWorkspaceSettingsGroups("acme", GUEST)).toEqual([]);
  });
});

describe("workspaceSettingsRoleLabel", () => {
  it("names each role and leaves an absent role blank", () => {
    expect(workspaceSettingsRoleLabel(ROLE_ADMIN)).toBe("Admin");
    expect(workspaceSettingsRoleLabel(ROLE_MEMBER)).toBe("Member");
    expect(workspaceSettingsRoleLabel(ROLE_GUEST)).toBe("Guest");
    expect(workspaceSettingsRoleLabel(null)).toBe("");
  });
});
