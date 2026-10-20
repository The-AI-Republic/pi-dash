// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { WITHDRAWN_TAB_FALLBACK, isProfileSettingsTabServed } from "./visibility.js";

describe("profile tab visibility", () => {
  it("withdraws the password tab only on a hosted instance", () => {
    expect(isProfileSettingsTabServed("security", false)).toBe(false);
    expect(isProfileSettingsTabServed("security", true)).toBe(true);
  });

  it("keeps the password tab while the instance answer is unknown", () => {
    expect(isProfileSettingsTabServed("security", undefined)).toBe(true);
  });

  it("serves every other tab on both instance kinds", () => {
    for (const selfManaged of [true, false, undefined]) {
      expect(isProfileSettingsTabServed("general", selfManaged)).toBe(true);
      expect(isProfileSettingsTabServed("ai-assistant", selfManaged)).toBe(true);
      expect(isProfileSettingsTabServed("api-tokens", selfManaged)).toBe(true);
    }
  });

  it("forwards a withdrawn address to the general tab", () => {
    expect(WITHDRAWN_TAB_FALLBACK).toBe("general");
  });
});
