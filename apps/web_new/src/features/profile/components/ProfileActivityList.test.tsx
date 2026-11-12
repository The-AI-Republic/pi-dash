// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Activity tab (parity/specs/profile.md §3, PROF-007): one hundred rows a
// page, a load-more control only while further pages exist, and no rows
// or control for a quiet member.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { IssueActivity, UserActivityPage } from "@pidash/api-client";
import { getUserProfileActivity } from "@pidash/api-client";

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, getUserProfileActivity: vi.fn() };
});

vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return {
    ...original,
    Link: (props: { to: string; children?: React.ReactNode }) => <a href={props.to}>{props.children}</a>,
  };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { PROFILE_ACTIVITY_PAGE_SIZE } from "../api/queries.js";
import { ProfileActivityTab } from "./ProfileActivityList.js";

function row(id: string, name: string): IssueActivity {
  return {
    id,
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
    issue_detail: { id: "i-1", name, sequence_id: Number(id.replace(/\D/g, "")) || 1 },
    project_detail: { id: "p-1", identifier: "PP", name: "Prof Proj" },
    workspace_detail: { id: "w-1", name: "Acme", slug: "acme" },
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
  };
}

function wrap(node: React.ReactElement): React.ReactElement {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } });
  return React.createElement(QueryClientProvider, { client }, node);
}

function page(results: IssueActivity[], next: string | null): UserActivityPage {
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: results.length,
    next_cursor: next ?? "",
    prev_cursor: "",
    next_page_results: next !== null,
    prev_page_results: false,
    count: results.length,
    total_pages: 1,
    total_results: results.length,
    extra_stats: null,
    results,
  };
}

beforeEach(() => {
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => {
  vi.restoreAllMocks();
  vi.resetAllMocks();
  resetAppClient();
});

describe("ProfileActivityTab", () => {
  it("renders the first page and no load-more control on the last page", () => {
    render(
      wrap(
        <ProfileActivityTab
          workspaceSlug="acme"
          userId="u-1"
          currentUserId="u-9"
          initialPage={page([row("a-1", "Prof one")], null)}
        />
      )
    );
    expect(screen.getAllByRole("listitem")).toHaveLength(1);
    expect(screen.queryByTestId("profile-activity-load-more")).toBeNull();
    expect(screen.getByRole("list", { name: "Activity" }).textContent).toContain("Prof one");
  });

  it("offers load-more while further pages exist and appends the next page", async () => {
    vi.mocked(getUserProfileActivity).mockResolvedValue(page([row("a-2", "Prof two")], null));
    render(
      wrap(
        <ProfileActivityTab
          workspaceSlug="acme"
          userId="u-1"
          currentUserId="u-9"
          initialPage={page([row("a-1", "Prof one")], "next-1")}
        />
      )
    );
    const more = screen.getByTestId("profile-activity-load-more");
    expect(more).toHaveTextContent("Load more");
    await userEvent.click(more);
    await waitFor(() => expect(screen.getAllByRole("listitem")).toHaveLength(2));
    expect(getUserProfileActivity).toHaveBeenCalledWith(expect.anything(), "acme", "u-1", {
      perPage: PROFILE_ACTIVITY_PAGE_SIZE,
      cursor: "next-1",
    });
    expect(PROFILE_ACTIVITY_PAGE_SIZE).toBe(100);
    expect(screen.queryByTestId("profile-activity-load-more")).toBeNull();
  });

  it("shows a working label and ignores a second click while loading", async () => {
    let release!: (value: UserActivityPage) => void;
    vi.mocked(getUserProfileActivity).mockReturnValue(
      new Promise<UserActivityPage>((resolve) => {
        release = resolve;
      })
    );
    render(
      wrap(
        <ProfileActivityTab
          workspaceSlug="acme"
          userId="u-1"
          currentUserId={null}
          initialPage={page([row("a-1", "Prof one")], "next-1")}
        />
      )
    );
    const more = screen.getByTestId("profile-activity-load-more");
    await userEvent.click(more);
    await waitFor(() => expect(more).toHaveTextContent("Loading…"));
    await userEvent.click(more);
    expect(getUserProfileActivity).toHaveBeenCalledTimes(1);
    release(page([row("a-2", "Prof two")], null));
    await waitFor(() => expect(screen.getAllByRole("listitem")).toHaveLength(2));
  });

  it("returns to idle when the next page read fails", async () => {
    vi.mocked(getUserProfileActivity).mockRejectedValue(new Error("down"));
    render(
      wrap(
        <ProfileActivityTab
          workspaceSlug="acme"
          userId="u-1"
          currentUserId={null}
          initialPage={page([row("a-1", "Prof one")], "next-1")}
        />
      )
    );
    await userEvent.click(screen.getByTestId("profile-activity-load-more"));
    await waitFor(() => expect(screen.getByTestId("profile-activity-load-more")).toHaveTextContent("Load more"));
    await waitFor(() => expect(getUserProfileActivity).toHaveBeenCalledTimes(1));
    expect(screen.getByTestId("profile-activity-load-more")).toBeEnabled();
    expect(screen.getAllByRole("listitem")).toHaveLength(1);
  });

  it("keeps the control on the following cursor when a page brings more", async () => {
    vi.mocked(getUserProfileActivity).mockResolvedValue(page([row("a-2", "Prof two")], "next-2"));
    render(
      wrap(
        <ProfileActivityTab
          workspaceSlug="acme"
          userId="u-1"
          currentUserId={null}
          initialPage={page([row("a-1", "Prof one")], "next-1")}
        />
      )
    );
    await userEvent.click(screen.getByTestId("profile-activity-load-more"));
    await waitFor(() => expect(screen.getAllByRole("listitem")).toHaveLength(2));
    expect(screen.getByTestId("profile-activity-load-more")).toBeInTheDocument();
    await userEvent.click(screen.getByTestId("profile-activity-load-more"));
    await waitFor(() =>
      expect(getUserProfileActivity).toHaveBeenLastCalledWith(expect.anything(), "acme", "u-1", {
        perPage: PROFILE_ACTIVITY_PAGE_SIZE,
        cursor: "next-2",
      })
    );
  });

  it("renders no rows and no control for a quiet member, keeping the download slot", () => {
    render(
      wrap(
        <ProfileActivityTab
          workspaceSlug="acme"
          userId="u-1"
          currentUserId={null}
          initialPage={page([], null)}
          download={<button type="button">Download today's activity</button>}
        />
      )
    );
    expect(screen.queryAllByRole("listitem")).toHaveLength(0);
    expect(screen.queryByTestId("profile-activity-load-more")).toBeNull();
    expect(screen.getByRole("button", { name: "Download today's activity" })).toBeInTheDocument();
  });

  it("drops the download slot when the caller withholds it", () => {
    render(
      wrap(<ProfileActivityTab workspaceSlug="acme" userId="u-1" currentUserId={null} initialPage={page([], null)} />)
    );
    expect(screen.queryByText("Download today's activity")).toBeNull();
    const list = screen.getByTestId("profile-activity-list");
    expect(within(list).queryAllByRole("listitem")).toHaveLength(0);
  });

  it("keeps the Recent-activity chrome with an empty list and no empty-state card", () => {
    render(
      wrap(<ProfileActivityTab workspaceSlug="acme" userId="u-1" currentUserId={null} initialPage={page([], null)} />)
    );
    expect(screen.getByRole("heading", { name: "Recent activity" })).toBeInTheDocument();
    expect(screen.queryByTestId("issues-empty-state")).toBeNull();
    expect(screen.getByTestId("profile-activity-list")).toBeInTheDocument();
  });
});
