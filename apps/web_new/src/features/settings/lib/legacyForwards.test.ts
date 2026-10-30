// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Legacy settings address families (parity/specs/profile.md §5, PROF-026;
// main#670). The pure resolver names the tab and the workspace a retained
// address stands for; the caller supplies the current workspace.
import { describe, expect, it } from "vitest";

import { legacySettingsTarget, legacyTabOf } from "./legacyForwards.js";

describe("legacyTabOf", () => {
  it("keeps a registered tab and defaults anything else", () => {
    expect(legacyTabOf("security")).toBe("security");
    expect(legacyTabOf("api-tokens")).toBe("api-tokens");
    expect(legacyTabOf("bogus-tab-id")).toBe("general");
    expect(legacyTabOf(undefined)).toBe("general");
  });
});

describe("legacySettingsTarget", () => {
  it("sends the bare settings root to the current workspace", () => {
    expect(legacySettingsTarget("/settings")).toEqual({ scope: "current", tab: "general" });
    expect(legacySettingsTarget("/settings/")).toEqual({ scope: "current", tab: "general" });
  });

  it("reads the tab after the legacy profile segment", () => {
    expect(legacySettingsTarget("/settings/profile")).toEqual({ scope: "current", tab: "general" });
    expect(legacySettingsTarget("/settings/profile/")).toEqual({ scope: "current", tab: "general" });
    expect(legacySettingsTarget("/settings/profile/preferences")).toEqual({
      scope: "current",
      tab: "preferences",
    });
    expect(legacySettingsTarget("/settings/profile/bogus-tab-id")).toEqual({ scope: "current", tab: "general" });
    // A deeper leftover is still a settings address: general, not a 404.
    expect(legacySettingsTarget("/settings/profile/security/extra")).toEqual({ scope: "current", tab: "general" });
    expect(legacySettingsTarget("/settings/account/security")).toEqual({ scope: "current", tab: "general" });
  });

  it("reads the older profile family", () => {
    expect(legacySettingsTarget("/profile")).toEqual({ scope: "current", tab: "general" });
    expect(legacySettingsTarget("/profile/")).toEqual({ scope: "current", tab: "general" });
    expect(legacySettingsTarget("/profile/security")).toEqual({ scope: "current", tab: "security" });
    expect(legacySettingsTarget("/profile/unknown")).toEqual({ scope: "current", tab: "general" });
    // Two segments below `/profile` is a member address, not a settings
    // one, and must fall through to the not-found page.
    expect(legacySettingsTarget("/profile/a/b")).toBeNull();
  });

  it("reads a workspace account address as that workspace's tab", () => {
    expect(legacySettingsTarget("/acme/settings/account")).toEqual({
      scope: "workspace",
      workspaceSlug: "acme",
      tab: "general",
    });
    expect(legacySettingsTarget("/acme/settings/account/security")).toEqual({
      scope: "workspace",
      workspaceSlug: "acme",
      tab: "security",
    });
    expect(legacySettingsTarget("/acme/settings/account/bogus")).toEqual({
      scope: "workspace",
      workspaceSlug: "acme",
      tab: "general",
    });
  });

  it("reads the workspace tokens address as that workspace's tokens tab", () => {
    expect(legacySettingsTarget("/acme/settings/api-tokens")).toEqual({
      scope: "workspace",
      workspaceSlug: "acme",
      tab: "api-tokens",
    });
    // A deeper tokens address is not the forward.
    expect(legacySettingsTarget("/acme/settings/api-tokens/more")).toBeNull();
  });

  it("ignores addresses outside the families", () => {
    expect(legacySettingsTarget("/")).toBeNull();
    expect(legacySettingsTarget("/acme/projects")).toBeNull();
    expect(legacySettingsTarget("/acme/settings")).toBeNull();
    expect(legacySettingsTarget("/acme/settings/members")).toBeNull();
    expect(legacySettingsTarget("/api/settings/account/security")).toBeNull();
  });
});
