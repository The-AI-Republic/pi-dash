// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Summary tab (parity/specs/profile.md §1, PROF-003/004/005): three count
// cards with tab hrefs and placeholders while stats load, the seven-group
// workload with zeros, priority and state charts or empty states, and
// recent activity with links and the compact empty state.
import { render, screen, within } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import type { IssueActivity, UserProfileStats } from "@pidash/api-client";

vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return {
    ...original,
    Link: (props: { to: string; children?: React.ReactNode; "data-profile-activity-link"?: string }) => (
      <a href={props.to} data-profile-activity-link={props["data-profile-activity-link"]}>
        {props.children}
      </a>
    ),
  };
});

import { PROFILE_OVERVIEW_ACTIVITY_EMPTY, ProfileOverview, overviewChartsEmpty } from "./ProfileOverview.js";

function stats(overrides: Partial<UserProfileStats> = {}): UserProfileStats {
  return {
    state_distribution: [
      { state_group: "started", state_count: 2 },
      { state_group: "completed", state_count: 1 },
    ],
    priority_distribution: [
      { priority: "urgent", priority_count: 3, priority_order: 0 },
      { priority: "low", priority_count: 1, priority_order: 3 },
    ],
    created_issues: 5,
    assigned_issues: 3,
    completed_issues: 1,
    pending_issues: 2,
    subscribed_issues: 4,
    present_cycles: [],
    upcoming_cycles: [],
    ...overrides,
  };
}

function activityRow(overrides: Partial<IssueActivity> = {}): IssueActivity {
  return {
    id: "a-1",
    issue: "i-1",
    project: "p-1",
    workspace: "w-1",
    actor: "u-1",
    actor_detail: {
      id: "u-1",
      first_name: "Opal",
      last_name: "Whelan",
      avatar: "",
      avatar_url: null,
      display_name: "Opal Five",
      is_bot: false,
    },
    issue_detail: { id: "i-1", name: "Prof acted", sequence_id: 12 },
    project_detail: { id: "p-1", identifier: "PP", name: "Prof Proj" },
    verb: "updated",
    field: "priority",
    old_value: "low",
    new_value: "urgent",
    comment: "",
    attachments: [],
    issue_comment: null,
    old_identifier: null,
    new_identifier: null,
    epoch: null,
    source_data: null,
    created_at: "2026-10-18T10:00:00Z",
    updated_at: "2026-10-18T10:00:00Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    ...overrides,
  };
}

function renderOverview(args: Partial<React.ComponentProps<typeof ProfileOverview>> = {}) {
  return render(
    <ProfileOverview
      workspaceSlug="acme"
      userId="u-1"
      stats={stats()}
      activity={[activityRow()]}
      currentUserId="u-9"
      {...args}
    />
  );
}

describe("ProfileOverview", () => {
  it("renders three placeholders while the stats read flies", () => {
    renderOverview({ stats: undefined });
    expect(screen.getAllByTestId("profile-card-placeholder")).toHaveLength(3);
    expect(screen.queryByTestId("profile-workload")).toBeNull();
  });

  it("renders the server totals with trailing-slash tab hrefs", () => {
    renderOverview();
    const cards = screen.getByTestId("profile-count-cards");
    const links = within(cards).getAllByRole("link");
    expect(links.map((link) => link.textContent)).toEqual([
      "Work items created5",
      "Work items assigned3",
      "Work items subscribed4",
    ]);
    expect(links.map((link) => link.getAttribute("href"))).toEqual([
      "/acme/profile/u-1/created/",
      "/acme/profile/u-1/assigned/",
      "/acme/profile/u-1/subscribed/",
    ]);
  });

  it("zero-fills the seven workload groups", () => {
    renderOverview();
    const groups = document.querySelectorAll("[data-profile-workload-group]");
    expect([...groups].map((group) => group.getAttribute("data-profile-workload-group"))).toEqual([
      "backlog",
      "unstarted",
      "started",
      "review",
      "test",
      "completed",
      "cancelled",
    ]);
    const started = document.querySelector('[data-profile-workload-group="started"]') as HTMLElement;
    const backlog = document.querySelector('[data-profile-workload-group="backlog"]') as HTMLElement;
    expect(started.textContent).toContain("Working on");
    expect(started.textContent).toContain("2");
    expect(backlog.textContent).toContain("Backlog");
    expect(backlog.textContent).toContain("0");
  });

  it("renders the priority and state breakdowns from the payload", () => {
    renderOverview();
    const priority = screen.getByTestId("profile-priority-chart");
    expect(
      within(priority)
        .getAllByRole("listitem")
        .map((row) => row.getAttribute("data-profile-priority"))
    ).toEqual(["Urgent", "Low"]);
    const legend = screen.getByTestId("profile-state-chart");
    expect(
      within(legend)
        .getAllByRole("listitem")
        .map((row) => row.getAttribute("data-profile-state-legend"))
    ).toEqual(["backlog", "unstarted", "started", "review", "test", "completed", "cancelled"]);
    const review = document.querySelector('[data-profile-state-legend="review"]') as HTMLElement;
    expect(review.textContent).toBe("Review0");
    expect(screen.queryByTestId("profile-chart-empty")).toBeNull();
  });

  it("shows both chart empty states for a member with nothing assigned", () => {
    renderOverview({
      stats: stats({ assigned_issues: 0, state_distribution: [], priority_distribution: [] }),
    });
    expect(screen.getAllByTestId("profile-chart-empty")).toHaveLength(2);
    expect(screen.queryByTestId("profile-priority-chart")).toBeNull();
    expect(screen.queryByTestId("profile-state-chart")).toBeNull();
    expect(overviewChartsEmpty(stats({ assigned_issues: 0, state_distribution: [], priority_distribution: [] }))).toBe(
      true
    );
    expect(overviewChartsEmpty(undefined)).toBe(false);
  });

  it("hides the zero-width bars when a series holds only zeros", () => {
    renderOverview({
      stats: stats({ priority_distribution: [{ priority: "low", priority_count: 0, priority_order: 3 }] }),
    });
    const bar = document.querySelector("[data-profile-priority='Low'] span span") as HTMLElement;
    expect(bar.style.width).toBe("0%");
  });

  it("keeps the state bar empty when every series count is zero", () => {
    renderOverview({
      stats: stats({
        assigned_issues: 0,
        state_distribution: [{ state_group: "canceled", state_count: 0 } as never],
        priority_distribution: [],
      }),
    });
    expect((document.querySelector("[data-profile-state-slice]") as HTMLElement).style.width).toBe("0%");
  });

  it("lists recent activity with browse links and names the actor", () => {
    renderOverview();
    const list = screen.getByTestId("profile-overview-activity");
    expect(within(list).getAllByRole("listitem")).toHaveLength(1);
    expect(list.textContent).toContain("Opal Five");
    expect(list.textContent).toContain("changed the priority");
    expect(list.textContent).toContain("Prof acted");
    expect(within(list).getByRole("link")).toHaveAttribute("href", "/acme/browse/PP-12");
  });

  it("renders a skeleton while activity loads and the compact empty state when quiet", () => {
    const { unmount } = renderOverview({ activity: undefined });
    expect(screen.queryByTestId("profile-overview-activity")).toBeNull();
    unmount();
    renderOverview({ activity: [] });
    expect(screen.getByTestId("profile-overview-activity-empty")).toHaveTextContent(PROFILE_OVERVIEW_ACTIVITY_EMPTY);
    expect(PROFILE_OVERVIEW_ACTIVITY_EMPTY).toBe("No Data yet");
    expect(screen.queryByTestId("profile-overview-activity")).toBeNull();
  });

  it("marks the signed-in member as You", () => {
    renderOverview({ currentUserId: "u-1" });
    expect(screen.getByTestId("profile-overview-activity").textContent).toContain("You");
  });

  it("leaves out a link whose work item was not serialised", () => {
    renderOverview({ activity: [activityRow({ issue_detail: null, project_detail: null })] });
    expect(screen.queryByRole("link", { name: "PP-12" })).toBeNull();
    const list = screen.getByTestId("profile-overview-activity");
    expect(list.textContent).toContain("changed the priority");
    expect(list.querySelector("a")).toBeNull();
  });
});
