// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  INTAKE_SOURCE_PILL_FLAG,
  ISSUE_DEDUPE_FLAG,
  PAGE_MOVE_FLAG,
  isAppRailEnabled,
  isPageMoveEnabled,
  isSidebarToggleEnabled,
  edition,
} from "./index.js";
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

  it("leaves the view access slot absent (VIEW-019: OSS renders no selector)", () => {
    expect(edition.slots?.viewAccess).toBeUndefined();
  });

  it("leaves the view publish slot absent (VIEW-020: OSS renders no publish entry)", () => {
    expect(edition.slots?.viewPublish).toBeUndefined();
  });

  it("leaves the workspace layouts slot absent (VIEW-040: OSS renders no selector)", () => {
    expect(edition.slots?.workspaceViewLayouts).toBeUndefined();
  });

  it("leaves the intake source pill flag absent (INT-034: OSS renders no source indicator)", () => {
    expect(edition.flags[INTAKE_SOURCE_PILL_FLAG]).toBeUndefined();
    expect(edition.flags[ISSUE_DEDUPE_FLAG]).toBeUndefined();
  });

  it("leaves the cross-project page move absent (PAGE-017: community pins no affordance)", () => {
    expect(edition.flags[PAGE_MOVE_FLAG]).toBeUndefined();
    expect(isPageMoveEnabled(edition)).toBe(false);
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

  it("enables the cross-project page move only where the edition opts in", () => {
    expect(isPageMoveEnabled(flags({}))).toBe(false);
    expect(isPageMoveEnabled(flags({ "page-move": false }))).toBe(false);
    expect(isPageMoveEnabled(flags({ "page-move": true }))).toBe(true);
  });

  it("mounts the sidebar toggle unless the edition opts out", () => {
    expect(isSidebarToggleEnabled(flags({}))).toBe(true);
    expect(isSidebarToggleEnabled(flags({ "sidebar-toggle": true }))).toBe(true);
    expect(isSidebarToggleEnabled(flags({ "sidebar-toggle": false }))).toBe(false);
    expect(isSidebarToggleEnabled(edition)).toBe(true);
  });
});
