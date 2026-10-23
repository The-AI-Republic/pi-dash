// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The list model (MOD-001/002/008): addresses, the tab title, the one body
// decision, the progress readings, the issue-count text and the default
// name order (§2.5).

import { describe, expect, it } from "vitest";

import {
  compareModuleNames,
  MODULES_CRUMB,
  moduleDetailHref,
  moduleIssueCountText,
  moduleProgressPercent,
  moduleProgressRatio,
  modulesFeatureHref,
  modulesListHref,
  modulesTitle,
  resolveModulesListBody,
  sortModulesByName,
} from "./moduleList.js";

describe("addresses and title", () => {
  it("builds the list, detail and feature-settings addresses", () => {
    expect(MODULES_CRUMB).toBe("Modules");
    expect(modulesListHref("acme", "p1")).toBe("/acme/projects/p1/modules/");
    expect(moduleDetailHref("acme", "p1", "m1")).toBe("/acme/projects/p1/modules/m1");
    expect(modulesFeatureHref("acme", "p1")).toBe("/acme/settings/projects/p1/features/modules");
  });

  it("reads the tab title as the project name followed by Modules", () => {
    expect(modulesTitle("Website relaunch")).toBe("Website relaunch - Modules");
  });
});

describe("resolveModulesListBody", () => {
  it("shows the gate over every list state once the project settles", () => {
    expect(resolveModulesListBody({ projectPending: false, gateOff: true, modulesPending: true })).toBe("gate");
    expect(resolveModulesListBody({ projectPending: false, gateOff: true, modulesPending: false })).toBe("gate");
  });

  it("shows the skeleton while the project or the list is loading", () => {
    expect(resolveModulesListBody({ projectPending: true, gateOff: false, modulesPending: true })).toBe("skeleton");
    expect(resolveModulesListBody({ projectPending: true, gateOff: true, modulesPending: false })).toBe("skeleton");
    expect(resolveModulesListBody({ projectPending: false, gateOff: false, modulesPending: true })).toBe("skeleton");
  });

  it("shows content once both reads settle", () => {
    expect(resolveModulesListBody({ projectPending: false, gateOff: false, modulesPending: false })).toBe("rows");
  });
});

describe("progress readings", () => {
  it("counts completed and cancelled over the total", () => {
    expect(moduleProgressRatio({ completed_issues: 1, cancelled_issues: 1, total_issues: 4 })).toBe(0.5);
    expect(moduleProgressPercent({ completed_issues: 1, cancelled_issues: 0, total_issues: 3 })).toBe(33);
  });

  it("reads a module without work items as zero", () => {
    expect(moduleProgressRatio({ completed_issues: 0, cancelled_issues: 0, total_issues: 0 })).toBe(0);
    expect(moduleProgressPercent({ completed_issues: 0, cancelled_issues: 0, total_issues: 0 })).toBe(0);
  });

  it("clamps a count the server reports beyond its own total", () => {
    expect(moduleProgressRatio({ completed_issues: 9, cancelled_issues: 9, total_issues: 4 })).toBe(1);
    expect(moduleProgressRatio({ completed_issues: -1, cancelled_issues: 0, total_issues: 4 })).toBe(0);
  });

  it("reads the card count as done over total", () => {
    expect(moduleIssueCountText({ completed_issues: 1, total_issues: 2 })).toBe("1/2");
  });
});

describe("compareModuleNames", () => {
  it("compares numeric runs by value", () => {
    expect(compareModuleNames("Module 2", "Module 10")).toBeLessThan(0);
    expect(compareModuleNames("Module 10", "Module 2")).toBeGreaterThan(0);
  });

  it("ignores case and keeps equal names equal", () => {
    expect(compareModuleNames("alpha", "Alpha")).toBe(0);
    expect(compareModuleNames("Delivery", "Delivery")).toBe(0);
  });

  it("compares text runs before a numeric tail", () => {
    expect(compareModuleNames("Alpha", "Alpha 1")).toBeLessThan(0);
    expect(compareModuleNames("Alpha 1", "Alpha")).toBeGreaterThan(0);
    expect(compareModuleNames("Alpha", "Beta")).toBeLessThan(0);
    expect(compareModuleNames("Beta", "Alpha")).toBeGreaterThan(0);
  });

  it("treats a missing match as an empty run", () => {
    // Both sides of the chunk regex always match here; empty inputs still
    // compare without throwing.
    expect(compareModuleNames("", "")).toBe(0);
    expect(compareModuleNames("", "a")).toBeLessThan(0);
  });
});

describe("sortModulesByName", () => {
  it("sorts naturally and stably without mutating the input", () => {
    const rows = [{ name: "Module 10" }, { name: "Module 2" }, { name: "Module 2" }, { name: "Alpha" }];
    const sorted = sortModulesByName(rows);
    expect(sorted.map((row) => row.name)).toEqual(["Alpha", "Module 2", "Module 2", "Module 10"]);
    expect(rows[0]?.name).toBe("Module 10");
  });
});
