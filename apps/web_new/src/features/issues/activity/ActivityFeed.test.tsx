// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Activity feed (ISS-194–196): one timeline over property changes and
// comments, filtered and sorted from the stored choices, with the
// composer placed above in newest-first and below in oldest-first.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { IssueActivity, IssueComment, TransportInit } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { ActivityFeed, FEED_LOAD_FAILED } from "./ActivityFeed.js";
import { FILTERS_STORAGE_KEY, SORT_STORAGE_KEY } from "./model.js";

const ISSUE = "123e4567-e89b-12d3-a456-426614174000";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ADA = "523e4567-e89b-12d3-a456-426614174004";

const ADA_DETAIL = {
  id: ADA,
  first_name: "Ada",
  last_name: "L",
  avatar: "",
  avatar_url: null,
  display_name: "Ada L",
  is_bot: false,
};

function activity(overrides: Partial<IssueActivity> = {}): IssueActivity {
  return {
    id: "a13e4567-e89b-12d3-a456-426614174001",
    issue: ISSUE,
    project: PROJECT,
    workspace: "423e4567-e89b-12d3-a456-426614174003",
    actor: ADA,
    actor_detail: ADA_DETAIL,
    verb: "created",
    field: null,
    old_value: null,
    new_value: null,
    comment: "",
    attachments: [],
    issue_comment: null,
    old_identifier: null,
    new_identifier: null,
    epoch: null,
    source_data: null,
    issue_detail: null,
    project_detail: null,
    workspace_detail: null,
    created_at: "2026-10-16T08:00:00Z",
    updated_at: "2026-10-16T08:00:00Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    ...overrides,
  };
}

function comment(overrides: Partial<IssueComment> = {}): IssueComment {
  return {
    id: "823e4567-e89b-12d3-a456-426614174008",
    issue: ISSUE,
    project: PROJECT,
    workspace: "423e4567-e89b-12d3-a456-426614174003",
    actor: ADA,
    actor_detail: ADA_DETAIL,
    comment_html: "<p>A comment</p>",
    comment_stripped: "A comment",
    comment_json: {},
    attachments: [],
    labels: [],
    access: "INTERNAL",
    edited_at: null,
    comment_reactions: [],
    speaker_type: "human",
    speaker_label: "",
    speaker_agent_run_id: null,
    parent: null,
    external_source: null,
    external_id: null,
    is_synced: false,
    description: null,
    created_at: "2026-10-16T10:00:00Z",
    updated_at: "2026-10-16T10:00:00Z",
    created_by: ADA,
    updated_by: null,
    deleted_at: null,
    ...overrides,
  };
}

function stubTransport(
  handler: (call: {
    method: string;
    url: string;
  }) => { status: number; body: unknown } | Promise<{ status: number; body: unknown }>
) {
  createAppClient("https://api.test", async (url: string, init: TransportInit) => {
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify({ csrf_token: "x" }) };
    }
    const answer = await handler({ method: init.method, url });
    return {
      status: answer.status,
      url,
      headers: { get: () => null },
      text: async () => (answer.body === "" ? "" : JSON.stringify(answer.body)),
    };
  });
}

interface RenderFeedProps {
  activities?: readonly IssueActivity[];
  comments?: readonly IssueComment[];
  activitiesStatus?: number;
  commentsStatus?: number;
  activitiesPending?: boolean;
  commentsPending?: boolean;
  archived?: boolean;
  worklogEnabled?: boolean;
  headerLeading?: React.ReactNode;
}

function renderFeed(props: RenderFeedProps = {}) {
  const {
    activities = [],
    comments = [],
    activitiesStatus = 200,
    commentsStatus = 200,
    activitiesPending = false,
    commentsPending = false,
    archived = false,
    worklogEnabled = false,
    headerLeading,
  } = props;
  stubTransport((call) => {
    if (call.url.includes("activity_type=issue-property")) {
      return activitiesPending ? new Promise(() => undefined) : { status: activitiesStatus, body: activities };
    }
    return commentsPending ? new Promise(() => undefined) : { status: commentsStatus, body: comments };
  });
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <ActivityFeed
        workspaceSlug="acme"
        projectId={PROJECT}
        issueId={ISSUE}
        comments={comments}
        commentsPending={commentsPending}
        commentsError={commentsStatus !== 200}
        composer={<form aria-label="Composer">Composer</form>}
        renderComment={(row) => <article data-testid={`comment-${row.id}`}>{row.comment_stripped}</article>}
        archived={archived}
        worklogEnabled={worklogEnabled}
        headerLeading={headerLeading}
      />
    </QueryClientProvider>
  );
}

function rows(): HTMLElement[] {
  return Array.from(document.querySelectorAll<HTMLElement>('[data-feed-row="true"]'));
}

beforeEach(() => {
  window.localStorage.clear();
});

afterEach(() => {
  resetAppClient();
  window.localStorage.clear();
});

describe("ActivityFeed", () => {
  it("shows three skeleton rows while the property read is in flight", async () => {
    renderFeed({ activitiesPending: true });
    expect(screen.getByRole("status", { name: "Loading activity" })).toBeInTheDocument();
    // Comments answer at once but the feed still waits for both.
    await waitFor(() => expect(screen.getByRole("status", { name: "Loading activity" })).toBeInTheDocument());
  });

  it("shows three skeleton rows while the comment read is in flight", async () => {
    renderFeed({ commentsPending: true });
    expect(screen.getByRole("status", { name: "Loading activity" })).toBeInTheDocument();
  });

  it("merges activities and comments oldest first by default", async () => {
    renderFeed({
      activities: [
        activity(),
        activity({
          id: "a13e4567-e89b-12d3-a456-426614174002",
          field: "state",
          verb: "updated",
          old_value: "Backlog",
          new_value: "In Progress",
          created_at: "2026-10-16T12:00:00Z",
        }),
      ],
      comments: [comment()],
    });
    await waitFor(() => expect(rows()).toHaveLength(3));
    expect(rows().map((row) => row.textContent)).toEqual([
      expect.stringContaining("created the work item."),
      expect.stringContaining("A comment"),
      expect.stringContaining("moved the state to In Progress."),
    ]);
  });

  it("sorts newest first once the stored order says so", async () => {
    window.localStorage.setItem(SORT_STORAGE_KEY, JSON.stringify("desc"));
    renderFeed({
      activities: [
        activity(),
        activity({
          id: "a13e4567-e89b-12d3-a456-426614174002",
          field: "state",
          verb: "updated",
          new_value: "In Progress",
          created_at: "2026-10-16T12:00:00Z",
        }),
      ],
    });
    await waitFor(() => expect(rows()).toHaveLength(2));
    expect(rows()[0]?.textContent).toContain("moved the state to In Progress.");
  });

  it("flips the order and remembers it when the sort button is pressed", async () => {
    const user = userEvent.setup();
    renderFeed({
      activities: [
        activity(),
        activity({
          id: "a13e4567-e89b-12d3-a456-426614174002",
          field: "state",
          verb: "updated",
          new_value: "In Progress",
          created_at: "2026-10-16T12:00:00Z",
        }),
      ],
    });
    await waitFor(() => expect(rows()).toHaveLength(2));
    await user.click(screen.getByRole("button", { name: /Change the activity order/ }));
    expect(rows()[0]?.textContent).toContain("moved the state to In Progress.");
    expect(window.localStorage.getItem(SORT_STORAGE_KEY)).toBe(JSON.stringify("desc"));
  });

  it("hides comments and property updates when their categories are off", async () => {
    const user = userEvent.setup();
    renderFeed({
      activities: [
        activity(),
        activity({
          id: "a13e4567-e89b-12d3-a456-426614174002",
          field: "state",
          verb: "updated",
          new_value: "In Progress",
          created_at: "2026-10-16T12:00:00Z",
        }),
      ],
      comments: [comment()],
    });
    await waitFor(() => expect(rows()).toHaveLength(3));
    await user.click(screen.getByRole("button", { name: "Filter activity" }));
    const group = await screen.findByRole("group", { name: "Activity categories" });
    await user.click(within(group).getByRole("checkbox", { name: "Comments" }));
    expect(rows()).toHaveLength(2);
    await user.click(within(group).getByRole("checkbox", { name: "State" }));
    expect(rows()).toHaveLength(1);
    expect(rows()[0]?.textContent).toContain("created the work item.");
    expect(JSON.parse(window.localStorage.getItem(FILTERS_STORAGE_KEY) ?? "null")).toEqual(["ACTIVITY", "ASSIGNEE"]);
  });

  it("hides field updates when the Updates category is off", async () => {
    const user = userEvent.setup();
    renderFeed({
      activities: [
        activity(),
        activity({
          id: "a13e4567-e89b-12d3-a456-426614174003",
          field: "description",
          verb: "updated",
          created_at: "2026-10-16T12:00:00Z",
        }),
      ],
    });
    await waitFor(() => expect(rows()).toHaveLength(2));
    await user.click(screen.getByRole("button", { name: "Filter activity" }));
    const group = await screen.findByRole("group", { name: "Activity categories" });
    await user.click(within(group).getByRole("checkbox", { name: "Updates" }));
    expect(rows()).toHaveLength(1);
    expect(rows()[0]?.textContent).toContain("created the work item.");
  });

  it("keeps the creation row and the last toggle even when its category is off", async () => {
    const user = userEvent.setup();
    renderFeed({ activities: [activity()] });
    await waitFor(() => expect(rows()).toHaveLength(1));
    await user.click(screen.getByRole("button", { name: "Filter activity" }));
    const group = await screen.findByRole("group", { name: "Activity categories" });
    await user.click(within(group).getByRole("checkbox", { name: "Updates" }));
    await user.click(within(group).getByRole("checkbox", { name: "Comments" }));
    await user.click(within(group).getByRole("checkbox", { name: "State" }));
    expect(rows()).toHaveLength(1);
    // The last category cannot be turned off; the creation row is never filtered.
    expect(within(group).getByRole("checkbox", { name: "Assignee" })).toHaveAttribute("aria-checked", "true");
  });

  it("puts the composer above the timeline in newest-first", async () => {
    window.localStorage.setItem(SORT_STORAGE_KEY, JSON.stringify("desc"));
    renderFeed({ comments: [comment()] });
    const composer = await screen.findByRole("form", { name: "Composer" });
    await waitFor(() => expect(rows()).toHaveLength(1));
    expect(composer.compareDocumentPosition(rows()[0] as Node) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("puts the composer below the timeline in oldest-first", async () => {
    renderFeed({ comments: [comment()] });
    const composer = await screen.findByRole("form", { name: "Composer" });
    await waitFor(() => expect(rows()).toHaveLength(1));
    const firstRow = rows()[0];
    expect(firstRow ? firstRow.compareDocumentPosition(composer) & Node.DOCUMENT_POSITION_FOLLOWING : 0).toBeTruthy();
  });

  it("drops the composer and keeps the timeline on an archived issue", async () => {
    renderFeed({ comments: [comment()], archived: true });
    await waitFor(() => expect(rows()).toHaveLength(1));
    expect(screen.queryByRole("form", { name: "Composer" })).not.toBeInTheDocument();
  });

  it("shows the read failure copy when either source is refused", async () => {
    renderFeed({ activitiesStatus: 500 });
    expect(await screen.findByText(FEED_LOAD_FAILED)).toBeInTheDocument();
  });

  it("shows no rows and no failure for an empty timeline", async () => {
    renderFeed();
    await waitFor(() => expect(screen.queryByRole("status", { name: "Loading activity" })).not.toBeInTheDocument());
    expect(rows()).toHaveLength(0);
    expect(screen.queryByText(FEED_LOAD_FAILED)).not.toBeInTheDocument();
  });

  it("renders the creation entry alongside the comments", async () => {
    renderFeed({ activities: [activity()], comments: [comment()] });
    expect(await screen.findByText("created the work item.")).toBeInTheDocument();
    expect(screen.getByTestId(`comment-${comment().id}`)).toBeInTheDocument();
  });

  it("renders worklog rows and narrows them through the fifth toggle in cloud", async () => {
    const user = userEvent.setup();
    renderFeed({
      activities: [
        activity(),
        activity({
          id: "b13e4567-e89b-12d3-a456-426614174002",
          field: "worklog",
          verb: "created",
          new_value: "90",
          created_at: "2026-10-16T12:00:00Z",
        }),
      ],
      worklogEnabled: true,
      headerLeading: <button type="button" aria-label="Log time" />,
    });
    await waitFor(() => expect(rows()).toHaveLength(2));
    expect(screen.getByText("logged 1h 30m.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Log time" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Filter activity" }));
    const group = await screen.findByRole("group", { name: "Activity categories" });
    expect(within(group).getAllByRole("checkbox")).toHaveLength(5);
    await user.click(within(group).getByRole("checkbox", { name: "Worklog" }));
    expect(rows()).toHaveLength(1);
    expect(screen.queryByText("logged 1h 30m.")).not.toBeInTheDocument();
  });

  it("keeps worklog rows off the OSS timeline with no fifth toggle", async () => {
    const user = userEvent.setup();
    renderFeed({
      activities: [
        activity(),
        activity({
          id: "b13e4567-e89b-12d3-a456-426614174002",
          field: "worklog",
          verb: "created",
          new_value: "90",
          created_at: "2026-10-16T12:00:00Z",
        }),
      ],
    });
    await waitFor(() => expect(rows()).toHaveLength(1));
    expect(screen.queryByText("logged 1h 30m.")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Filter activity" }));
    const group = await screen.findByRole("group", { name: "Activity categories" });
    expect(within(group).getAllByRole("checkbox")).toHaveLength(4);
  });
});
