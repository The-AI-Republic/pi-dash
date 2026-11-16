// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Legacy settings address families (parity/specs/profile.md §5, PROF-026;
// main#670). The pure resolver names the tab a retained workspace-less
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
  it("sends the bare settings root to the general tab", () => {
    expect(legacySettingsTarget("/settings")).toBe("general");
    expect(legacySettingsTarget("/settings/")).toBe("general");
  });

  it("reads the tab after the legacy profile segment", () => {
    expect(legacySettingsTarget("/settings/profile")).toBe("general");
    expect(legacySettingsTarget("/settings/profile/")).toBe("general");
    expect(legacySettingsTarget("/settings/profile/preferences")).toBe("preferences");
    expect(legacySettingsTarget("/settings/profile/bogus-tab-id")).toBe("general");
    // A deeper leftover is still a settings address: general, not a 404.
    expect(legacySettingsTarget("/settings/profile/security/extra")).toBe("general");
    expect(legacySettingsTarget("/settings/account/security")).toBe("general");
  });

  it("reads the older profile family", () => {
    expect(legacySettingsTarget("/profile")).toBe("general");
    expect(legacySettingsTarget("/profile/")).toBe("general");
    expect(legacySettingsTarget("/profile/security")).toBe("security");
    expect(legacySettingsTarget("/profile/unknown")).toBe("general");
    // Two segments below `/profile` is a member address, not a settings
    // one, and must fall through to the not-found page.
    expect(legacySettingsTarget("/profile/a/b")).toBeNull();
  });

  it("ignores addresses outside the workspace-less families", () => {
    expect(legacySettingsTarget("/")).toBeNull();
    expect(legacySettingsTarget("/acme/projects")).toBeNull();
    // Workspace addresses are routes that forward themselves.
    expect(legacySettingsTarget("/acme/settings/account/security")).toBeNull();
    expect(legacySettingsTarget("/acme/settings/api-tokens")).toBeNull();
    expect(legacySettingsTarget("/acme/settings")).toBeNull();
    expect(legacySettingsTarget("/acme/settings/members")).toBeNull();
    expect(legacySettingsTarget("/api/settings/account/security")).toBeNull();
  });
});
