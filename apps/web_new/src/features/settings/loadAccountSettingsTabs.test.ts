// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Lazy door to the built-in tab bodies (PROF-009/010; main#670): the first
// ask registers them, later asks reuse one request so tab navigation never
// re-registers, and a failed ask can be tried again.
import { afterEach, describe, expect, it, vi } from "vitest";

import { getProfileSettingsPage, resetProfileSettingsPages } from "./lib/registry.js";
import { loadAccountSettingsTabs, resetAccountSettingsTabsLoader } from "./loadAccountSettingsTabs.js";

const attempts = vi.hoisted(() => ({ count: 0 }));

afterEach(() => {
  resetAccountSettingsTabsLoader();
  resetProfileSettingsPages();
  vi.doUnmock("./pageEntries.js");
});

describe("loadAccountSettingsTabs", () => {
  it("registers the built-in bodies before it resolves", async () => {
    expect(getProfileSettingsPage("general")).toBeNull();
    await loadAccountSettingsTabs();
    expect(getProfileSettingsPage("general")).not.toBeNull();
  });

  it("reuses one request across visits, settled or still in flight", async () => {
    const inFlight = loadAccountSettingsTabs();
    expect(loadAccountSettingsTabs()).toBe(inFlight);
    await inFlight;
    expect(loadAccountSettingsTabs()).toBe(inFlight);
  });

  it("forgets a failed request so the next ask tries again", async () => {
    attempts.count = 0;
    vi.doMock("./pageEntries.js", () => {
      attempts.count += 1;
      if (attempts.count === 1) throw new Error("chunk unavailable");
      return { registerBuiltInSettingsTabs: (): (() => void) => (): void => {} };
    });
    // Vitest wraps a throwing mock factory in its own error; the point here
    // is that the ask rejects and the memo does not keep the failure.
    await expect(loadAccountSettingsTabs()).rejects.toBeInstanceOf(Error);
    await expect(loadAccountSettingsTabs()).resolves.toBeUndefined();
    expect(attempts.count).toBe(2);
  });
});
