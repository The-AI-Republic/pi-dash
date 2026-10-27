// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Dashboard shell pure logic: entry table, prefix highlight, crumb
// trail, and collapse persistence.
import { afterEach, describe, expect, it } from "vitest";

import {
  ADMIN_CRUMB_HOME,
  ADMIN_NARROW_VIEWPORT_PX,
  ADMIN_SEGMENT_LABELS,
  ADMIN_SIDEBAR_COLLAPSED_KEY,
  ADMIN_SIDEBAR_ENTRIES,
  adminSegmentLabel,
  buildAdminCrumbs,
  GOD_MODE_MOUNT,
  isSidebarEntryActive,
  readStoredSidebarCollapsed,
  writeStoredSidebarCollapsed,
} from "./adminShell.js";

describe("sidebar entries", () => {
  it("lists the seven settings areas in order with no image entry", () => {
    expect(ADMIN_SIDEBAR_ENTRIES.map((entry) => entry.name)).toEqual([
      "General",
      "Email",
      "Authentication",
      "Workspaces",
      "Artificial intelligence",
      "Loop",
      "Runners",
    ]);
    expect(ADMIN_SIDEBAR_ENTRIES.some((entry) => entry.slug === "image")).toBe(false);
  });

  it("gives every entry a description and a canonical console href", () => {
    for (const entry of ADMIN_SIDEBAR_ENTRIES) {
      expect(entry.description).not.toBe("");
      expect(entry.href).toBe(`${GOD_MODE_MOUNT}/${entry.slug}/`);
    }
  });

  it("keeps stable unique keys", () => {
    const keys = ADMIN_SIDEBAR_ENTRIES.map((entry) => entry.key);
    expect(new Set(keys).size).toBe(keys.length);
  });
});

describe("isSidebarEntryActive", () => {
  it("matches the entry's own page with or without the trailing slash", () => {
    expect(isSidebarEntryActive("/god-mode/ai/", "/god-mode/ai/")).toBe(true);
    expect(isSidebarEntryActive("/god-mode/ai", "/god-mode/ai/")).toBe(true);
  });

  it("keeps the parent highlighted on sub-pages", () => {
    expect(isSidebarEntryActive("/god-mode/authentication/github/", "/god-mode/authentication/")).toBe(true);
    expect(isSidebarEntryActive("/god-mode/workspace/create/", "/god-mode/workspace/")).toBe(true);
  });

  it("never matches siblings or segment prefixes", () => {
    expect(isSidebarEntryActive("/god-mode/email/", "/god-mode/ai/")).toBe(false);
    expect(isSidebarEntryActive("/god-mode/ai-extra/", "/god-mode/ai/")).toBe(false);
    expect(isSidebarEntryActive("/god-mode/", "/god-mode/general/")).toBe(false);
    expect(isSidebarEntryActive("/", "/god-mode/general/")).toBe(false);
  });
});

describe("adminSegmentLabel", () => {
  it("maps the known settings areas, providers, and the create segment", () => {
    expect(ADMIN_SEGMENT_LABELS).toEqual({
      general: "General",
      ai: "Artificial Intelligence",
      email: "Email",
      authentication: "Authentication",
      google: "Google",
      github: "GitHub",
      gitlab: "GitLab",
      gitea: "Gitea",
      workspace: "Workspace",
      create: "Create",
    });
  });

  it("carries no image label, since the console has no image area (ADM-049)", () => {
    expect(Object.keys(ADMIN_SEGMENT_LABELS)).not.toContain("image");
    expect(adminSegmentLabel("image")).toBe("IMAGE");
  });

  it("uppercases unmapped segments", () => {
    expect(adminSegmentLabel("loop")).toBe("LOOP");
    expect(adminSegmentLabel("runners")).toBe("RUNNERS");
    expect(adminSegmentLabel("abc123")).toBe("ABC123");
  });
});

describe("buildAdminCrumbs", () => {
  it("leads with the settings crumb back to general settings", () => {
    expect(ADMIN_CRUMB_HOME).toEqual({ label: "Settings", href: "/god-mode/general/" });
    const [lead] = buildAdminCrumbs("/god-mode/general/");
    expect(lead).toEqual({ label: "Settings", href: "/god-mode/general/" });
  });

  it("renders one linked crumb per segment with cumulative paths", () => {
    expect(buildAdminCrumbs("/god-mode/workspace/create/")).toEqual([
      { label: "Settings", href: "/god-mode/general/" },
      { label: "Workspace", href: "/god-mode/workspace/" },
      { label: "Create", href: "/god-mode/workspace/create/" },
    ]);
    expect(buildAdminCrumbs("/god-mode/authentication/github/")).toEqual([
      { label: "Settings", href: "/god-mode/general/" },
      { label: "Authentication", href: "/god-mode/authentication/" },
      { label: "GitHub", href: "/god-mode/authentication/github/" },
    ]);
  });

  it("falls back to uppercase for unmapped segments and raw ids", () => {
    expect(buildAdminCrumbs("/god-mode/loop/abc123/")).toEqual([
      { label: "Settings", href: "/god-mode/general/" },
      { label: "LOOP", href: "/god-mode/loop/" },
      { label: "ABC123", href: "/god-mode/loop/abc123/" },
    ]);
  });

  it("tolerates slashless addresses and bare mount-relative paths", () => {
    expect(buildAdminCrumbs("/god-mode/general")).toEqual([
      { label: "Settings", href: "/god-mode/general/" },
      { label: "General", href: "/god-mode/general/" },
    ]);
    expect(buildAdminCrumbs("/general/")).toEqual([
      { label: "Settings", href: "/god-mode/general/" },
      { label: "General", href: "/god-mode/general/" },
    ]);
  });
});

describe("collapse persistence", () => {
  afterEach(() => {
    window.localStorage.clear();
  });

  it("pins the narrow-viewport breakpoint and the storage key", () => {
    expect(ADMIN_NARROW_VIEWPORT_PX).toBe(768);
    expect(ADMIN_SIDEBAR_COLLAPSED_KEY).toBe("god_mode_sidebar_collapsed");
  });

  it("reads expanded when nothing is stored", () => {
    expect(readStoredSidebarCollapsed()).toBe(false);
  });

  it("round-trips the flag", () => {
    writeStoredSidebarCollapsed(true);
    expect(window.localStorage.getItem(ADMIN_SIDEBAR_COLLAPSED_KEY)).toBe("true");
    expect(readStoredSidebarCollapsed()).toBe(true);
    writeStoredSidebarCollapsed(false);
    expect(window.localStorage.getItem(ADMIN_SIDEBAR_COLLAPSED_KEY)).toBe("false");
    expect(readStoredSidebarCollapsed()).toBe(false);
  });

  it("reads expanded for unknown stored values", () => {
    window.localStorage.setItem(ADMIN_SIDEBAR_COLLAPSED_KEY, "yes");
    expect(readStoredSidebarCollapsed()).toBe(false);
  });
});
