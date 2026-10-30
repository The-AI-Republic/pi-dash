// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Account-settings addresses (PROF-009/010/026; main#670): one workspace,
// one tab per address, query kept verbatim, trailing slash on every link.
import { describe, expect, it } from "vitest";

import {
  accountSettingsGeneralHref,
  accountSettingsHref,
  accountSettingsWorkspace,
  isAccountSettingsPath,
} from "./accountHref.js";

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

describe("isAccountSettingsPath", () => {
  it("recognizes every tab of any workspace", () => {
    expect(isAccountSettingsPath("/acme/settings/account/security/")).toBe(true);
    expect(isAccountSettingsPath("/acme/settings/account/security")).toBe(true);
    expect(isAccountSettingsPath("/acme/settings/account/general/", "acme")).toBe(true);
  });

  it("narrows to one workspace when asked", () => {
    expect(isAccountSettingsPath("/acme/settings/account/general/", "globex")).toBe(false);
  });

  it("rejects workspace settings and unrelated addresses", () => {
    expect(isAccountSettingsPath("/acme/settings/members/")).toBe(false);
    expect(isAccountSettingsPath("/acme/settings")).toBe(false);
    expect(isAccountSettingsPath("/settings/profile/general")).toBe(false);
    expect(isAccountSettingsPath("/")).toBe(false);
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
