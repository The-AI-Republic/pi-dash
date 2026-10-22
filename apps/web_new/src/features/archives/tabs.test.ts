// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the archives tab model (ARCH-001–003): fixed order,
// flag gating, addresses and per-tab titles.
import { describe, expect, it } from "vitest";

import {
  ARCHIVES_TAB_DEFS,
  archivesRouteTitle,
  archivesTabDef,
  archivesTabHref,
  archivesTabTitle,
  selectArchivesTabs,
} from "./tabs.js";

describe("ARCHIVES_TAB_DEFS", () => {
  it("lists work items, cycles and modules in fixed order", () => {
    expect(ARCHIVES_TAB_DEFS.map((def) => def.label)).toEqual(["Work items", "Cycles", "Modules"]);
    expect(ARCHIVES_TAB_DEFS.map((def) => def.key)).toEqual(["issues", "cycles", "modules"]);
  });
});

describe("archivesTabHref", () => {
  it("addresses each tab under the project archives tree", () => {
    expect(archivesTabHref("acme", "p1", "issues")).toBe("/acme/projects/p1/archives/issues");
    expect(archivesTabHref("acme", "p1", "cycles")).toBe("/acme/projects/p1/archives/cycles");
    expect(archivesTabHref("acme", "p1", "modules")).toBe("/acme/projects/p1/archives/modules");
  });
});

describe("selectArchivesTabs", () => {
  it("renders all three tabs when both flags are on", () => {
    const tabs = selectArchivesTabs({
      workspaceSlug: "acme",
      projectId: "p1",
      flags: { cycle_view: true, module_view: true },
    });

    expect(tabs.map((tab) => tab.label)).toEqual(["Work items", "Cycles", "Modules"]);
    expect(tabs.map((tab) => tab.href)).toEqual([
      "/acme/projects/p1/archives/issues",
      "/acme/projects/p1/archives/cycles",
      "/acme/projects/p1/archives/modules",
    ]);
  });

  it("keeps only the work-items tab when both flags are off", () => {
    const tabs = selectArchivesTabs({
      workspaceSlug: "acme",
      projectId: "p1",
      flags: { cycle_view: false, module_view: false },
    });

    expect(tabs.map((tab) => tab.label)).toEqual(["Work items"]);
  });

  it("hides just the disabled tab when one flag is off", () => {
    const withoutCycles = selectArchivesTabs({
      workspaceSlug: "acme",
      projectId: "p1",
      flags: { cycle_view: false, module_view: true },
    });
    const withoutModules = selectArchivesTabs({
      workspaceSlug: "acme",
      projectId: "p1",
      flags: { cycle_view: true, module_view: false },
    });

    expect(withoutCycles.map((tab) => tab.label)).toEqual(["Work items", "Modules"]);
    expect(withoutModules.map((tab) => tab.label)).toEqual(["Work items", "Cycles"]);
  });
});

describe("archivesTabDef", () => {
  it("returns the catalogue entry per key", () => {
    expect(archivesTabDef("issues")).toEqual({
      key: "issues",
      label: "Work items",
      section: "work items",
      flag: null,
    });
    expect(archivesTabDef("cycles").label).toBe("Cycles");
    expect(archivesTabDef("modules").label).toBe("Modules");
  });
});

describe("archivesTabTitle", () => {
  it("names the project plus the archive section per tab", () => {
    expect(archivesTabTitle("Acme project", "issues")).toBe("Acme project - Archived work items");
    expect(archivesTabTitle("Acme project", "cycles")).toBe("Acme project - Archived cycles");
    expect(archivesTabTitle("Acme project", "modules")).toBe("Acme project - Archived modules");
  });
});

describe("archivesRouteTitle", () => {
  it("falls back while the project row resolves", () => {
    expect(archivesRouteTitle(undefined, "issues")).toBe("Archives");
    expect(archivesRouteTitle("Acme project", "cycles")).toBe("Acme project - Archived cycles");
  });
});
