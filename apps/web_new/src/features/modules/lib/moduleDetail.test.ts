// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The module detail screen model (MOD-029): the tab title shape, the
// switcher pick target, the gone-away predicate and the pinned badge
// tooltip in both numbers.

import { ApiError } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  isModuleGoneAway,
  moduleBadgeTooltip,
  moduleDetailTitle,
  projectIssuesHref,
  switchModuleTarget,
} from "./moduleDetail.js";

describe("moduleDetailTitle", () => {
  it("reads project followed by module", () => {
    expect(moduleDetailTitle("Acme", "Alpha")).toBe("Acme - Alpha");
  });
});

describe("projectIssuesHref", () => {
  it("points at the project's issues screen", () => {
    expect(projectIssuesHref("acme", "p1")).toBe("/acme/projects/p1/issues");
  });
});

describe("switchModuleTarget", () => {
  it("navigates a fresh pick to its detail address", () => {
    expect(switchModuleTarget("acme", "p1", "m1", "m2")).toBe("/acme/projects/p1/modules/m2");
  });

  it("stays put on a cleared or repeated pick", () => {
    expect(switchModuleTarget("acme", "p1", "m1", null)).toBe(null);
    expect(switchModuleTarget("acme", "p1", "m1", "m1")).toBe(null);
  });
});

describe("isModuleGoneAway", () => {
  it("treats an archived record as gone", () => {
    expect(isModuleGoneAway(null, "2026-01-01T00:00:00Z")).toBe(true);
  });

  it("treats a live 404 as gone", () => {
    const notFound = new ApiError({ status: 404, code: "http", message: "missing" });
    expect(isModuleGoneAway(notFound, null)).toBe(true);
  });

  it("keeps other failures and live records on the page", () => {
    const serverError = new ApiError({ status: 500, code: "http", message: "boom" });
    expect(isModuleGoneAway(serverError, null)).toBe(false);
    expect(isModuleGoneAway(new Error("boom"), null)).toBe(false);
    expect(isModuleGoneAway(null, null)).toBe(false);
    expect(isModuleGoneAway(null, undefined)).toBe(false);
  });
});

describe("moduleBadgeTooltip", () => {
  it("pins the plural tooltip", () => {
    expect(moduleBadgeTooltip(2)).toBe("There are 2 work items in this module");
    expect(moduleBadgeTooltip(0)).toBe("There are 0 work items in this module");
  });

  it("uses the singular noun for one item", () => {
    expect(moduleBadgeTooltip(1)).toBe("There are 1 work item in this module");
  });
});
