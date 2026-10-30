// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Account-settings addresses (PROF-009/010/026; main#670): one workspace,
// one tab per address, query kept verbatim, trailing slash on every link.
import { describe, expect, it } from "vitest";

import { accountSettingsGeneralHref, accountSettingsHref, accountSettingsWorkspace } from "./accountHref.js";

describe("accountSettingsHref", () => {
  it("names the tab inside the workspace with a trailing slash", () => {
    expect(accountSettingsHref("acme", "security")).toBe("/acme/settings/account/security/");
    expect(accountSettingsGeneralHref("acme")).toBe("/acme/settings/account/general/");
  });

  it("appends a fragment after the query when one is given", () => {
    expect(accountSettingsHref("acme", "ai-assistant", "", "voice-dictation")).toBe(
      "/acme/settings/account/ai-assistant/#voice-dictation"
    );
    expect(accountSettingsHref("acme", "ai-assistant", "?x=1", "#voice-dictation")).toBe(
      "/acme/settings/account/ai-assistant/?x=1#voice-dictation"
    );
    expect(accountSettingsHref("acme", "general", "", "")).toBe("/acme/settings/account/general/");
  });

  it("keeps a query verbatim whether or not it carries its mark", () => {
    expect(accountSettingsHref("acme", "general", "?keep=1")).toBe("/acme/settings/account/general/?keep=1");
    expect(accountSettingsHref("acme", "general", "keep=1")).toBe("/acme/settings/account/general/?keep=1");
    expect(accountSettingsHref("acme", "general", "?")).toBe("/acme/settings/account/general/");
    expect(accountSettingsHref("acme", "general", "")).toBe("/acme/settings/account/general/");
  });
});

describe("accountSettingsWorkspace", () => {
  it("reads the workspace the tab belongs to", () => {
    expect(accountSettingsWorkspace("/acme/settings/account/security/")).toBe("acme");
    expect(accountSettingsWorkspace("/globex/settings/account/")).toBe("globex");
  });

  it("answers null outside account settings", () => {
    expect(accountSettingsWorkspace("/acme/settings/members/")).toBeNull();
    expect(accountSettingsWorkspace("/settings/profile/general/")).toBeNull();
  });
});
