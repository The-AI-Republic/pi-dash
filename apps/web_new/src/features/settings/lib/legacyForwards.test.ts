// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { legacySettingsForward } from "./legacyForwards.js";
import { legacyRedirectOptions } from "./legacyRedirect.js";

describe("legacy settings forwards", () => {
  it("sends the bare settings address to general with parameters kept", () => {
    expect(legacySettingsForward("/settings", "")).toEqual({
      tab: "general",
      query: "",
      href: "/settings/profile/general/",
    });
    expect(legacySettingsForward("/settings/", "?keep=1&other=2")).toEqual({
      tab: "general",
      query: "keep=1&other=2",
      href: "/settings/profile/general/?keep=1&other=2",
    });
  });

  it("forwards a direct profile tab address, defaulting unknown ids", () => {
    expect(legacySettingsForward("/settings/profile/security", "")?.href).toBe("/settings/profile/security/");
    expect(legacySettingsForward("/settings/profile/bogus-tab-id", "")?.href).toBe("/settings/profile/general/");
    // Deeper addresses are not settings forwards.
    expect(legacySettingsForward("/settings/profile/security/extra", "")).toBeNull();
    expect(legacySettingsForward("/settings/projects/p1", "")).toBeNull();
  });

  it("forwards the legacy /profile family", () => {
    expect(legacySettingsForward("/profile", "")?.href).toBe("/settings/profile/general/");
    expect(legacySettingsForward("/profile/", "")?.href).toBe("/settings/profile/general/");
    expect(legacySettingsForward("/profile/preferences", "?keep=4")).toEqual({
      tab: "preferences",
      query: "keep=4",
      href: "/settings/profile/preferences/?keep=4",
    });
    expect(legacySettingsForward("/profile/unknown", "")?.href).toBe("/settings/profile/general/");
    expect(legacySettingsForward("/profile/a/b", "")).toBeNull();
  });

  it("forwards workspace account addresses per tab, defaulting to general", () => {
    expect(legacySettingsForward("/acme/settings/account/", "")?.href).toBe("/settings/profile/general/");
    expect(legacySettingsForward("/acme/settings/account/security", "?keep=2")).toEqual({
      tab: "security",
      query: "keep=2",
      href: "/settings/profile/security/?keep=2",
    });
    expect(legacySettingsForward("/acme/settings/account/bogus", "")?.href).toBe("/settings/profile/general/");
    expect(legacySettingsForward("/acme/settings/account/a/b", "")).toBeNull();
  });

  it("forwards the workspace api-tokens address with its parameters (NEWFRONT-431 target)", () => {
    expect(legacySettingsForward("/acme/settings/api-tokens", "?keep=3")).toEqual({
      tab: "api-tokens",
      query: "keep=3",
      href: "/settings/profile/api-tokens/?keep=3",
    });
    expect(legacySettingsForward("/acme/settings/api-tokens", "")?.href).toBe("/settings/profile/api-tokens/");
    expect(legacySettingsForward("/acme/settings/api-tokens/more", "")).toBeNull();
  });

  it("ignores addresses outside the four families", () => {
    expect(legacySettingsForward("/", "")).toBeNull();
    expect(legacySettingsForward("/acme/projects", "")).toBeNull();
    expect(legacySettingsForward("/acme/settings", "")).toBeNull();
    expect(legacySettingsForward("/acme/settings/members", "")).toBeNull();
  });

  it("builds router redirect options for a forward", () => {
    expect(legacyRedirectOptions("/acme/settings/account/security", "?keep=2")).toEqual({
      to: "/settings/profile/security/?keep=2",
      search: true,
    });
    expect(legacyRedirectOptions("/acme/projects", "")).toBeNull();
  });
});
