// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Profile reads (parity/specs/profile.md §1–4): each option builder calls
// the matching contract with the tab's ownership selector and page size.
import { describe, expect, it, vi } from "vitest";

import type { ApiClient } from "@pidash/api-client";
import {
  getUserProfileActivity,
  getUserProfileSegregation,
  getUserProfileStats,
  listUserProfileIssues,
} from "@pidash/api-client";

import { profileKeys } from "./keys.js";
import {
  PROFILE_ACTIVITY_PAGE_SIZE,
  PROFILE_ISSUES_PAGE_SIZE,
  PROFILE_OVERVIEW_ACTIVITY_SIZE,
  profileActivityQueryOptions,
  profileIssuesQueryOptions,
  profileSegregationQueryOptions,
  profileStatsQueryOptions,
} from "./queries.js";

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return {
    ...original,
    getUserProfileActivity: vi.fn(),
    getUserProfileSegregation: vi.fn(),
    getUserProfileStats: vi.fn(),
    listUserProfileIssues: vi.fn(),
  };
});

const client = {} as ApiClient;

describe("profile query options", () => {
  it("pins the page sizes the spec states", () => {
    expect(PROFILE_ISSUES_PAGE_SIZE).toBe(100);
    expect(PROFILE_ACTIVITY_PAGE_SIZE).toBe(100);
    expect(PROFILE_OVERVIEW_ACTIVITY_SIZE).toBe(10);
  });

  it("reads segregation and stats under their member keys", async () => {
    const segregation = profileSegregationQueryOptions(client, "acme", "u-1");
    const stats = profileStatsQueryOptions(client, "acme", "u-1");
    expect(segregation.queryKey).toEqual(profileKeys.segregation("acme", "u-1"));
    expect(stats.queryKey).toEqual(profileKeys.stats("acme", "u-1"));
    await segregation.queryFn?.({} as never);
    await stats.queryFn?.({} as never);
    expect(getUserProfileSegregation).toHaveBeenCalledWith(client, "acme", "u-1");
    expect(getUserProfileStats).toHaveBeenCalledWith(client, "acme", "u-1");
  });

  it("reads one ownership-selected page with the chosen order", async () => {
    const options = profileIssuesQueryOptions(client, "acme", "u-1", "created", "priority");
    expect(options.queryKey.slice(0, -1)).toEqual(profileKeys.issues("acme", "u-1", "created"));
    expect(options.queryKey[options.queryKey.length - 1]).toBe("priority");
    await options.queryFn?.({} as never);
    expect(listUserProfileIssues).toHaveBeenCalledWith(client, "acme", "u-1", {
      ownership: "created",
      perPage: 100,
      orderBy: "priority",
    });
  });

  it("reads the first activity page at the requested size", async () => {
    const options = profileActivityQueryOptions(client, "acme", "u-1", 10);
    expect(options.queryKey).toEqual(profileKeys.activity("acme", "u-1", 10));
    await options.queryFn?.({} as never);
    expect(getUserProfileActivity).toHaveBeenCalledWith(client, "acme", "u-1", { perPage: 10 });
  });
});
