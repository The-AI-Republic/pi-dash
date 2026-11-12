// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Hero reads key under the shared cycle keys and fetch through the
// api-client contracts (CYC-041).

import { describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  listCycleRows: vi.fn(),
  getCycleProgress: vi.fn(),
  getCycleAnalytics: vi.fn(),
  listCycleIssues: vi.fn(),
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return {
    ...original,
    listCycleRows: mocks.listCycleRows,
    getCycleProgress: mocks.getCycleProgress,
    getCycleAnalytics: mocks.getCycleAnalytics,
    listCycleIssues: mocks.listCycleIssues,
  };
});

import {
  HERO_ISSUES_PAGE_SIZE,
  heroAnalyticsQueryOptions,
  heroIssuesQueryOptions,
  heroProgressQueryOptions,
  heroQueryOptions,
} from "./heroQueries.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const CYCLE = "323e4567-e89b-12d3-a456-426614174002";
const CLIENT = { stub: true } as never;

describe("heroQueryOptions", () => {
  it("reads the current-scoped list under the hero key", async () => {
    mocks.listCycleRows.mockResolvedValue([]);
    const options = heroQueryOptions(CLIENT, "acme", PROJECT);
    expect(options.queryKey).toEqual(["ws", "acme", "cycles", "hero", PROJECT]);
    await options.queryFn?.({} as never);
    expect(mocks.listCycleRows).toHaveBeenCalledWith(CLIENT, "acme", PROJECT, { cycleView: "current" });
  });
});

describe("heroProgressQueryOptions", () => {
  it("reads the progress snapshot under the progress key", async () => {
    mocks.getCycleProgress.mockResolvedValue({});
    const options = heroProgressQueryOptions(CLIENT, "acme", PROJECT, CYCLE);
    expect(options.queryKey).toEqual(["ws", "acme", "cycles", "progress", PROJECT, CYCLE]);
    await options.queryFn?.({} as never);
    expect(mocks.getCycleProgress).toHaveBeenCalledWith(CLIENT, "acme", PROJECT, CYCLE);
  });
});

describe("heroAnalyticsQueryOptions", () => {
  it("reads the distribution per measure under the analytics key", async () => {
    mocks.getCycleAnalytics.mockResolvedValue({});
    const issues = heroAnalyticsQueryOptions(CLIENT, "acme", PROJECT, CYCLE, "issues");
    expect(issues.queryKey).toEqual(["ws", "acme", "cycles", "analytics", PROJECT, CYCLE, "issues"]);
    await issues.queryFn?.({} as never);
    expect(mocks.getCycleAnalytics).toHaveBeenCalledWith(CLIENT, "acme", PROJECT, CYCLE, "issues");
    const points = heroAnalyticsQueryOptions(CLIENT, "acme", PROJECT, CYCLE, "points");
    expect(points.queryKey).toEqual(["ws", "acme", "cycles", "analytics", PROJECT, CYCLE, "points"]);
  });
});

describe("heroIssuesQueryOptions", () => {
  it("reads the first issues page under the hero-issues key", async () => {
    mocks.listCycleIssues.mockResolvedValue({});
    const options = heroIssuesQueryOptions(CLIENT, "acme", PROJECT, CYCLE);
    expect(options.queryKey).toEqual(["ws", "acme", "cycles", "hero-issues", PROJECT, CYCLE]);
    await options.queryFn?.({} as never);
    expect(mocks.listCycleIssues).toHaveBeenCalledWith(CLIENT, "acme", PROJECT, CYCLE, {
      perPage: HERO_ISSUES_PAGE_SIZE,
    });
  });
});
