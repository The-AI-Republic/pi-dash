// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Built-in tab registration (parity/specs/profile.md §5): the general,
// security and notifications bodies are present on a direct visit, and
// unmounting them leaves the seam to the sibling slices. The layout's
// session guard is shared with the app shell (AUTH-020).
import { render } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { getProfileSettingsPage, resetProfileSettingsPages } from "./lib/registry.js";
import { registerBuiltInSettingsTabs, useProfileSettingsSessionGuard } from "./pageEntries.js";

vi.mock("../../shared/shell/expiredRedirect.js", () => ({
  useSessionExpiredRedirect: vi.fn(),
}));

import { useSessionExpiredRedirect } from "../../shared/shell/expiredRedirect.js";

afterEach(() => resetProfileSettingsPages());

describe("built-in settings tabs", () => {
  it("registers the general, security and notifications bodies", () => {
    expect(getProfileSettingsPage("general")).not.toBeNull();
    expect(getProfileSettingsPage("security")).not.toBeNull();
    expect(getProfileSettingsPage("notifications")).not.toBeNull();
    expect(getProfileSettingsPage("activity")).toBeNull();
  });

  it("unregisters all three on demand", () => {
    const unregister = registerBuiltInSettingsTabs();
    unregister();
    expect(getProfileSettingsPage("general")).toBeNull();
    expect(getProfileSettingsPage("security")).toBeNull();
    expect(getProfileSettingsPage("notifications")).toBeNull();
  });

  it("routes the settings layout through the session-expiry redirect", () => {
    function Probe(): React.ReactElement | null {
      useProfileSettingsSessionGuard();
      return null;
    }
    render(<Probe />);
    expect(useSessionExpiredRedirect).toHaveBeenCalledTimes(1);
  });
});
