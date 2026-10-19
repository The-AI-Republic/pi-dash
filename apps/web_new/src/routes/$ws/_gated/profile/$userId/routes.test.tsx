// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Profile route files (parity/specs/profile.md §1–§4): the layout warms
// the identity and stats reads and mounts the frame; the summary renders
// the overview; the tab leaf renders the ownership list, denies guests
// the three issue tabs, leaves the activity feed open without its
// download, and renders nothing for an unknown tab id.
import { QueryClient } from "@tanstack/react-query";
import { screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { UserActivityPage, UserProfileSegregation, UserProfileStats } from "@pidash/api-client";
import {
  getUserProfileActivity,
  getUserProfileSegregation,
  getUserProfileStats,
  listProjects,
  listUserProfileIssues,
} from "@pidash/api-client";

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return {
    ...original,
    getUserProfileActivity: vi.fn(),
    getUserProfileSegregation: vi.fn(),
    getUserProfileStats: vi.fn(),
    listProjects: vi.fn(),
    listUserProfileIssues: vi.fn(),
  };
});

vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return {
    ...original,
    Link: (props: { to: string; children?: React.ReactNode }) => <a href={props.to}>{props.children}</a>,
  };
});

import { resetAppClient } from "../../../../../core/api/client.js";
import { sessionKeys } from "../../../../../core/session/index.js";
import { projectsQueryOptions } from "../../../../../features/projects/index.js";
import { profileKeys } from "../../../../../features/profile/api/keys.js";
import {
  profileActivityQueryOptions,
  profileSegregationQueryOptions,
  profileStatsQueryOptions,
} from "../../../../../features/profile/api/queries.js";
import { renderProfileRoute } from "./-profileHarness.js";
import { Route as TabRoute } from "./$tabId.js";
import { Route as SummaryRoute } from "./index.js";
import { Route as LayoutRoute } from "./route.js";

const segregation: UserProfileSegregation = {
  user_data: {
    email: "opal@example.com",
    first_name: "Opal",
    last_name: "Whelan",
    display_name: "Opal W.",
    avatar_url: null,
    cover_image_url: null,
    date_joined: "2026-03-09T12:00:00Z",
    user_timezone: "UTC",
  },
  project_data: [],
};

const stats: UserProfileStats = {
  state_distribution: [],
  priority_distribution: [],
  created_issues: 1,
  assigned_issues: 0,
  completed_issues: 0,
  pending_issues: 0,
  subscribed_issues: 2,
  present_cycles: [],
  upcoming_cycles: [],
};

function issuePage() {
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: 0,
    next_cursor: "",
    prev_cursor: "",
    next_page_results: false,
    prev_page_results: false,
    count: 0,
    total_results: 0,
    results: [],
  };
}

function activityPage(): UserActivityPage {
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: 0,
    next_cursor: "",
    prev_cursor: "",
    next_page_results: false,
    prev_page_results: false,
    count: 0,
    total_pages: 0,
    total_results: 0,
    extra_stats: null,
    results: [],
  };
}

function seed(role: number, meId = "u-9") {
  return (queryClient: QueryClient) => {
    queryClient.setQueryData(sessionKeys.me, { id: meId });
    queryClient.setQueryData(sessionKeys.workspaces, [
      { id: "w-1", name: "Acme", slug: "acme", logo_url: null, total_members: 3, role },
    ]);
  };
}

/** Role present, member identity still in flight. */
function roleWithoutMember(queryClient: QueryClient): void {
  queryClient.setQueryData(sessionKeys.workspaces, [
    { id: "w-1", name: "Acme", slug: "acme", logo_url: null, total_members: 3, role: 15 },
  ]);
}

beforeEach(() => {
  vi.mocked(getUserProfileSegregation).mockResolvedValue(segregation);
  vi.mocked(getUserProfileStats).mockResolvedValue(stats);
  vi.mocked(getUserProfileActivity).mockResolvedValue(activityPage());
  vi.mocked(listProjects).mockResolvedValue([]);
  vi.mocked(listUserProfileIssues).mockResolvedValue(issuePage() as never);
});

afterEach(() => {
  resetAppClient();
  vi.clearAllMocks();
});

describe("profile layout route", () => {
  it("warms the segregation and stats reads and tolerates a failed stats call", async () => {
    const loader = LayoutRoute.options.loader as unknown as (input: unknown) => Promise<unknown>;
    const queryClient = new QueryClient();
    vi.mocked(getUserProfileStats).mockRejectedValue(new Error("stats down"));
    await loader({ context: { queryClient, apiClient: {} }, params: { ws: "acme", userId: "u-1" } });
    expect(getUserProfileSegregation).toHaveBeenCalledWith({}, "acme", "u-1");
    expect(getUserProfileStats).toHaveBeenCalledWith({}, "acme", "u-1");
    expect(queryClient.getQueryData(profileKeys.segregation("acme", "u-1"))).toEqual(segregation);
  });

  it("warms both reads and mounts the frame for the member", async () => {
    const loader = LayoutRoute.options.loader as unknown as (input: unknown) => Promise<unknown>;
    const queryClient = new QueryClient();
    await loader({ context: { queryClient, apiClient: {} }, params: { ws: "acme", userId: "u-1" } });
    expect(queryClient.getQueryData(profileKeys.stats("acme", "u-1"))).toEqual(stats);
    renderProfileRoute({
      component: LayoutRoute.options.component as never,
      leafPath: "/",
      seed: (client) => {
        seed(15)(client);
        client.setQueryData(profileKeys.segregation("acme", "u-1"), segregation);
        client.setQueryData(profileKeys.stats("acme", "u-1"), stats);
        client.setQueryData(profileKeys.activity("acme", "u-1", 10), activityPage());
        client.setQueryData(projectsQueryOptions({} as never, "acme").queryKey, []);
      },
    });
    expect(await screen.findByTestId("profile-page")).toBeInTheDocument();
  });

  it("mounts the frame while the project list is still in flight", async () => {
    renderProfileRoute({
      component: LayoutRoute.options.component as never,
      leafPath: "/",
      seed: (client) => {
        seed(15)(client);
        client.setQueryData(profileKeys.segregation("acme", "u-1"), segregation);
        client.setQueryData(profileKeys.stats("acme", "u-1"), stats);
        client.setQueryData(profileKeys.activity("acme", "u-1", 10), activityPage());
      },
    });
    expect(await screen.findByTestId("profile-page")).toBeInTheDocument();
    expect(screen.getByTestId("profile-sidebar")).toBeInTheDocument();
  });
});

describe("profile summary route", () => {
  it("warms stats and activity and renders the overview", async () => {
    const loader = SummaryRoute.options.loader as unknown as (input: unknown) => Promise<unknown>;
    const queryClient = new QueryClient();
    await loader({ context: { queryClient, apiClient: {} }, params: { ws: "acme", userId: "u-1" } });
    expect(getUserProfileStats).toHaveBeenCalled();
    expect(getUserProfileActivity).toHaveBeenCalledWith({}, "acme", "u-1", { perPage: 10 });
    expect(queryClient.getQueryData(profileKeys.stats("acme", "u-1"))).toEqual(stats);
  });

  it("renders the count cards and the tab strip", async () => {
    renderProfileRoute({
      component: SummaryRoute.options.component as never,
      leafPath: "/",
      seed: (queryClient) => {
        seed(15)(queryClient);
        queryClient.setQueryData(profileSegregationQueryOptions({} as never, "acme", "u-1").queryKey, segregation);
        queryClient.setQueryData(profileStatsQueryOptions({} as never, "acme", "u-1").queryKey, stats);
        queryClient.setQueryData(projectsQueryOptions({} as never, "acme").queryKey, []);
      },
    });
    expect(await screen.findByTestId("profile-count-cards")).toBeInTheDocument();
  });
});

describe("profile tab route", () => {
  it("reads the activity page in the loader and ignores a failed read", async () => {
    const loader = TabRoute.options.loader as unknown as (input: unknown) => Promise<unknown>;
    const queryClient = new QueryClient();
    vi.mocked(getUserProfileActivity).mockRejectedValue(new Error("down"));
    await expect(
      loader({ context: { queryClient, apiClient: {} }, params: { ws: "acme", userId: "u-1", tabId: "activity" } })
    ).resolves.toEqual({});
    vi.clearAllMocks();
    await loader({ context: { queryClient, apiClient: {} }, params: { ws: "acme", userId: "u-1", tabId: "assigned" } });
    expect(getUserProfileActivity).not.toHaveBeenCalled();
  });
});

describe("profile tab body", () => {
  const memberSeed = (queryClient: QueryClient) => {
    seed(15)(queryClient);
    queryClient.setQueryData(sessionKeys.me, { id: "u-9" });
  };
  const guestSeed = (queryClient: QueryClient) => {
    seed(5)(queryClient);
  };

  function mountTab(tabId: string, role: number) {
    return renderProfileRoute({
      component: TabRoute.options.component as never,
      leafPath: "$tabId",
      initialPath: `/acme/profile/u-1/${tabId}`,
      seed: role === 5 ? guestSeed : memberSeed,
    });
  }

  it("renders the ownership list for a member on an issue tab", async () => {
    mountTab("created", 15);
    expect(await screen.findByTestId("profile-issues-tab")).toHaveAttribute("data-profile-ownership", "created");
    expect(listUserProfileIssues).toHaveBeenCalledWith(
      expect.anything(),
      "acme",
      "u-1",
      expect.objectContaining({ ownership: "created" })
    );
  });

  it.each(["assigned", "created", "subscribed"])("denies a guest the %s tab", async (tab) => {
    mountTab(tab, 5);
    expect(await screen.findByTestId("profile-access-denied")).toBeInTheDocument();
    expect(screen.queryByTestId("profile-issues-tab")).toBeNull();
  });

  it("shows a guest the activity feed without the download", async () => {
    mountTab("activity", 5);
    expect(await screen.findByTestId("profile-activity")).toBeInTheDocument();
    expect(screen.queryByTestId("profile-access-denied")).toBeNull();
    expect(screen.queryByTestId("profile-activity-download")).toBeNull();
  });

  it("shows a member the activity feed with the download control", async () => {
    mountTab("activity", 15);
    expect(await screen.findByTestId("profile-activity-download")).toBeInTheDocument();
  });

  it("renders nothing for an unknown tab id", async () => {
    const { router } = mountTab("bogus", 15);
    await waitFor(() => expect(router.state.location.pathname).toBe("/acme/profile/u-1/bogus"));
    expect(screen.queryByTestId("profile-access-denied")).toBeNull();
    expect(screen.queryByTestId("profile-issues-tab")).toBeNull();
    expect(screen.queryByTestId("profile-activity")).toBeNull();
  });

  it("renders the activity tab for a session whose member read has not landed", async () => {
    renderProfileRoute({
      component: TabRoute.options.component as never,
      leafPath: "$tabId",
      initialPath: "/acme/profile/u-1/activity",
      seed: roleWithoutMember,
    });
    expect(await screen.findByTestId("profile-activity")).toBeInTheDocument();
  });

  it("hands the activity tab its first page while it is still loading", async () => {
    vi.mocked(getUserProfileActivity).mockReturnValue(new Promise(() => undefined));
    mountTab("activity", 15);
    expect(await screen.findByTestId("profile-activity")).toBeInTheDocument();
    expect(screen.queryByTestId("profile-activity-load-more")).toBeNull();
  });

  it("hands the activity tab the warmed first page instead of refetching it", async () => {
    renderProfileRoute({
      component: TabRoute.options.component as never,
      leafPath: "$tabId",
      initialPath: "/acme/profile/u-1/activity",
      seed: (client) => {
        memberSeed(client);
        client.setQueryData(profileActivityQueryOptions({} as never, "acme", "u-1", 100).queryKey, activityPage());
      },
    });
    expect(await screen.findByTestId("profile-activity")).toBeInTheDocument();
    expect(getUserProfileActivity).not.toHaveBeenCalled();
  });

  it("renders the summary for a session whose member read has not landed", async () => {
    renderProfileRoute({ component: SummaryRoute.options.component as never, leafPath: "/", seed: roleWithoutMember });
    expect(await screen.findByTestId("profile-count-cards")).toBeInTheDocument();
  });

  it("renders the summary once the overview reads land", async () => {
    renderProfileRoute({
      component: SummaryRoute.options.component as never,
      leafPath: "/",
      seed: (queryClient) => {
        seed(15)(queryClient);
        queryClient.setQueryData(profileStatsQueryOptions({} as never, "acme", "u-1").queryKey, stats);
        queryClient.setQueryData(profileActivityQueryOptions({} as never, "acme", "u-1", 10).queryKey, activityPage());
      },
    });
    expect(await screen.findByTestId("profile-count-cards")).toBeInTheDocument();
    expect(screen.getByTestId("profile-count-cards")).toHaveTextContent("1");
  });
});
