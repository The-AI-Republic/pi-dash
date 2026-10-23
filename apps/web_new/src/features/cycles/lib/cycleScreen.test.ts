// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycles screen model: addresses, tab title, and the body-state order
// (CYC-010–012).

import { describe, expect, it } from "vitest";

import { ApiError } from "@pidash/api-client";
import {
  CYCLES_CRUMB,
  cycleDetailPath,
  cyclesFeatureHref,
  cyclesListHref,
  cyclesTitle,
  isCycleGoneAway,
  projectIssuesHref,
  switchCycleTarget,
  resolveCyclesBody,
  type CyclesBodyInput,
} from "./cycleScreen.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";

function input(overrides: Partial<CyclesBodyInput> = {}): CyclesBodyInput {
  return {
    projectPending: false,
    projectError: false,
    gateOff: false,
    cyclesPending: false,
    cyclesError: false,
    cycleCount: 2,
    ...overrides,
  };
}

describe("cycles screen model", () => {
  it("names the crumb and the tab title", () => {
    expect(CYCLES_CRUMB).toBe("Cycles");
    expect(cyclesTitle("Website relaunch")).toBe("Website relaunch - Cycles");
  });

  it("addresses the list and the cycles feature-settings leaf", () => {
    expect(cyclesListHref("acme", PROJECT)).toBe(`/acme/projects/${PROJECT}/cycles/`);
    expect(cyclesFeatureHref("acme", PROJECT)).toBe(`/acme/settings/projects/${PROJECT}/features/cycles`);
  });

  it("addresses one cycle's detail without a trailing slash", () => {
    expect(cycleDetailPath("acme", PROJECT, "c1")).toBe(`/acme/projects/${PROJECT}/cycles/c1`);
  });

  it("addresses the project work-items page for the detail trail", () => {
    expect(projectIssuesHref("acme", PROJECT)).toBe(`/acme/projects/${PROJECT}/issues`);
  });

  it("stays put on a cleared or repeated switcher pick", () => {
    expect(switchCycleTarget("acme", PROJECT, "c1", null)).toBe(null);
    expect(switchCycleTarget("acme", PROJECT, "c1", "c1")).toBe(null);
    expect(switchCycleTarget("acme", PROJECT, "c1", "c2")).toBe(`/acme/projects/${PROJECT}/cycles/c2`);
  });

  it("shows the skeleton while either read is pending", () => {
    expect(resolveCyclesBody(input({ projectPending: true }))).toBe("skeleton");
    expect(resolveCyclesBody(input({ cyclesPending: true, cycleCount: 0 }))).toBe("skeleton");
  });

  it("shows the error when a read fails", () => {
    expect(resolveCyclesBody(input({ projectError: true }))).toBe("error");
    expect(resolveCyclesBody(input({ cyclesError: true, cycleCount: 0 }))).toBe("error");
  });

  it("gates over every list state when the switch is off", () => {
    expect(resolveCyclesBody(input({ gateOff: true, cycleCount: 0 }))).toBe("gate");
    expect(resolveCyclesBody(input({ gateOff: true, cyclesError: true }))).toBe("gate");
  });

  it("splits zero from rows on the fetched count", () => {
    expect(resolveCyclesBody(input({ cycleCount: 0 }))).toBe("zero");
    expect(resolveCyclesBody(input({ cycleCount: 3 }))).toBe("list");
  });

  it("gone-aways a 404 or an archived-via-live read only", () => {
    const notFound = new ApiError({ status: 404, code: "http", message: "missing" });
    const forbidden = new ApiError({ status: 403, code: "http", message: "denied" });
    expect(isCycleGoneAway(notFound, null)).toBe(true);
    expect(isCycleGoneAway(null, "2026-09-01T00:00:00Z")).toBe(true);
    expect(isCycleGoneAway(forbidden, null)).toBe(false);
    expect(isCycleGoneAway(forbidden, undefined)).toBe(false);
    expect(isCycleGoneAway(new Error("boom"), null)).toBe(false);
    expect(isCycleGoneAway(null, null)).toBe(false);
  });
});
