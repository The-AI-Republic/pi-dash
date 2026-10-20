// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { isAppRailEnabled, isSidebarToggleEnabled, edition } from "./index.js";
import { edition as ossEdition } from "./oss.js";
import type { Edition } from "./types.js";

describe("edition", () => {
  it("resolves the OSS edition through the index", () => {
    expect(edition).toBe(ossEdition);
    expect(edition.id).toBe("oss");
  });

  it("contributes no cloud surface", () => {
    expect(edition.routes ?? []).toEqual([]);
    expect(edition.sidebar ?? []).toEqual([]);
    expect(edition.rail ?? []).toEqual([]);
    expect(edition.settingsSections ?? []).toEqual([]);
    // The four native providers are backend-native, not a cloud extra: the
    // instance flags alone decide which buttons show (AUTH-003).
    expect(edition.auth ?? []).toEqual([
      { id: "google", label: "Google" },
      { id: "github", label: "GitHub" },
      { id: "gitlab", label: "GitLab" },
      { id: "gitea", label: "Gitea" },
    ]);
    expect(edition.api ?? []).toEqual([]);
    expect(edition.flags).toEqual({});
    expect(edition.slots ?? {}).toEqual({});
  });

  it("leaves the cloud god-mode entry absent (ADM-074: OSS renders the bootstrap gate)", () => {
    expect(edition.slots?.godModeCloudEntry).toBeUndefined();
  });

  it("leaves the cloud login card absent (CLOUD-002: OSS keeps /login's redirect)", () => {
    expect(edition.slots?.cloudLoginCard).toBeUndefined();
  });
});

describe("shell edition gates", () => {
  const flags = (entries: Record<string, boolean>): Edition => ({ id: "test", flags: entries });

  it("suppresses the rail unless the edition opts in", () => {
    expect(isAppRailEnabled(flags({}))).toBe(false);
    expect(isAppRailEnabled(flags({ "app-rail": false }))).toBe(false);
    expect(isAppRailEnabled(flags({ "app-rail": true }))).toBe(true);
    expect(isAppRailEnabled(edition)).toBe(false);
  });

  it("mounts the sidebar toggle unless the edition opts out", () => {
    expect(isSidebarToggleEnabled(flags({}))).toBe(true);
    expect(isSidebarToggleEnabled(flags({ "sidebar-toggle": true }))).toBe(true);
    expect(isSidebarToggleEnabled(flags({ "sidebar-toggle": false }))).toBe(false);
    expect(isSidebarToggleEnabled(edition)).toBe(true);
  });
});
