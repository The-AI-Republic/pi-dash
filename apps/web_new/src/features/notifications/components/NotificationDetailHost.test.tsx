// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Issue, TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { ROLE_GUEST } from "../../../core/session/index.js";
import type { NotificationCardData } from "../lib/card.js";
import { NotificationDetailHost } from "./NotificationDetailHost.js";

vi.mock("../../issues/index.js", async () => {
  const actual = await vi.importActual<typeof import("../../issues/index.js")>("../../issues/index.js");
  return {
    ...actual,
    IssueDetailRoot: ({ issue, editable }: { issue: Issue; editable: boolean }) => (
      <div data-testid="issue-detail-root" data-editable={editable ? "true" : "false"}>
        {issue.name}
      </div>
    ),
    // The seam's own read-only rendering is covered by its component test;
    // here it proves the host routes triage selections through it.
    TriageDetailRoot: ({ issue, reference }: { issue: Issue; reference: string }) => (
      <div data-testid="issue-triage-root" data-reference={reference}>
        {issue.name}
      </div>
    ),
  };
});

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";

function selection(overrides: Partial<NotificationCardData> = {}): NotificationCardData {
  return {
    id: "523e4567-e89b-12d3-a456-426614174004",
    issueId: ISSUE,
    projectId: PROJECT,
    actorName: "Ada Lovelace",
    actorAvatarUrl: null,
    summary: "Ada Lovelace commented: detail note",
    reference: "ENG-12",
    title: "Ship it",
    createdAt: "2026-10-17T12:00:00Z",
    unread: false,
    mentioned: false,
    triage: false,
    archivedAt: null,
    snoozedTill: null,
    streamFlags: { is_inbox_issue: false, is_intake_issue: false, is_mentioned_notification: false },
    ...overrides,
  };
}

function issue(): Issue {
  return {
    id: ISSUE,
    name: "Ship it",
    state_id: null,
    sort_order: 1,
    completed_at: null,
    estimate_point: null,
    priority: "high",
    sequence_id: 12,
    project_id: PROJECT,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2026-10-17T12:00:00Z",
    updated_at: "2026-10-17T12:00:00Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    complexity_score: 0,
    agent_executor: null,
    is_synced: false,
    description_html: "<p>Detail</p>",
    is_subscribed: false,
    agent_ticker: null,
    agent_status: null,
    relations_summary: { blocked_by: [], blocking: [] },
    has_open_blockers: false,
  };
}

function response(url: string, body: unknown, status = 200): TransportResponse {
  return {
    status,
    url,
    headers: { get: () => null },
    text: async () => JSON.stringify(body),
  } as TransportResponse;
}

/** The per-project access read the pane gates on. */
function isAccess(url: string): boolean {
  return url.endsWith(`/projects/${PROJECT}/project-members/me/`);
}

function renderHost(value: NotificationCardData | null, onClose = vi.fn()) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return {
    ...render(
      <QueryClientProvider client={queryClient}>
        <NotificationDetailHost workspaceSlug="acme" selection={value} onClose={onClose} />
      </QueryClientProvider>
    ),
    onClose,
  };
}

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
});

describe("NotificationDetailHost", () => {
  it("renders a neutral placeholder without starting access reads", () => {
    const transport = vi.fn(async (url: string) => response(url, {}));
    createAppClient("https://api.test", transport as never);
    renderHost(null);
    expect(screen.getByTestId("notifications-detail-placeholder")).toHaveTextContent("Choose an update");
    expect(screen.getByTestId("notifications-detail-pane")).not.toHaveAttribute("data-detail-variant");
    expect(transport).not.toHaveBeenCalled();
  });

  it("shows access and issue loading, then the editable peek inside the pane and closes from it", async () => {
    const user = userEvent.setup();
    let releaseAccess!: () => void;
    let releaseIssue!: () => void;
    const accessGate = new Promise<void>((resolve) => {
      releaseAccess = resolve;
    });
    const issueGate = new Promise<void>((resolve) => {
      releaseIssue = resolve;
    });
    createAppClient("https://api.test", (async (url: string) => {
      if (url.endsWith(`/projects/${PROJECT}/project-members/me/`)) {
        await accessGate;
        return response(url, { id: "923e4567-e89b-12d3-a456-426614174009", role: 15 });
      }
      await issueGate;
      return response(url, issue());
    }) as never);
    const { onClose } = renderHost(selection());
    expect(screen.getByTestId("notifications-detail-loading")).toBeInTheDocument();

    releaseAccess();
    await waitFor(() => expect(screen.getByTestId("notifications-detail-loading")).toBeInTheDocument());
    releaseIssue();
    await waitFor(() => expect(screen.getByTestId("issue-detail-root")).toHaveAttribute("data-editable", "true"));

    const pane = screen.getByTestId("notifications-detail-pane");
    expect(pane).toHaveAttribute("data-detail-variant", "peek");
    expect(within(pane).getByTestId("notifications-detail-summary")).toHaveTextContent(
      "Ada Lovelace commented: detail note"
    );
    expect(within(pane).getByTestId("notifications-detail-actor")).toHaveTextContent("Ada Lovelace");
    expect(within(pane).getByText("ENG-12")).toBeInTheDocument();
    expect(within(pane).getByTestId("issue-detail-root")).toHaveTextContent("Ship it");
    expect(within(pane).getByText("Work item preview")).toBeInTheDocument();
    expect(screen.queryByTestId("issue-preview")).not.toBeInTheDocument();

    await user.click(within(pane).getByTestId("notifications-detail-close"));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("shows the loading state while the per-project access read is in flight", async () => {
    let releaseAccess!: () => void;
    const accessGate = new Promise<void>((resolve) => {
      releaseAccess = resolve;
    });
    const seen: string[] = [];
    createAppClient("https://api.test", (async (url: string) => {
      seen.push(url);
      if (isAccess(url)) {
        await accessGate;
        return response(url, { id: "923e4567-e89b-12d3-a456-426614174009", role: 15 });
      }
      return response(url, issue());
    }) as never);
    renderHost(selection());
    expect(screen.getByTestId("notifications-detail-loading")).toBeInTheDocument();
    expect(seen.some((url) => isAccess(url))).toBe(true);
    expect(seen.some((url) => url.includes("/project-roles/"))).toBe(false);
    releaseAccess();
    await waitFor(() =>
      expect(screen.getByTestId("notifications-detail-pane")).toHaveAttribute("data-detail-variant", "peek")
    );
    expect(screen.queryByTestId("notifications-detail-loading")).not.toBeInTheDocument();
  });

  it("routes triage selections to read-only issue-owned content", async () => {
    createAppClient("https://api.test", (async (url: string) =>
      isAccess(url)
        ? response(url, { id: "923e4567-e89b-12d3-a456-426614174009", role: 20 })
        : response(url, issue())) as never);
    renderHost(selection({ triage: true }));
    await within(await screen.findByTestId("notifications-detail-pane")).findByTestId("issue-triage-root");
    const pane = screen.getByTestId("notifications-detail-pane");
    expect(pane).toHaveAttribute("data-detail-variant", "triage");
    expect(within(pane).getByText("Triage work item")).toBeInTheDocument();
    expect(within(pane).getByTestId("notifications-detail-summary")).toHaveTextContent(
      "Ada Lovelace commented: detail note"
    );
    expect(within(pane).getByTestId("notifications-detail-close")).toBeInTheDocument();
    // The triage branch renders the issues-owned triage content with the
    // selected item, not the ordinary detail root.
    expect(within(pane).getByTestId("issue-triage-root")).toHaveTextContent("Ship it");
    expect(within(pane).getByTestId("issue-triage-root")).toHaveAttribute("data-reference", "ENG-12");
    expect(within(pane).queryByTestId("issue-detail-root")).not.toBeInTheDocument();
  });

  it("keeps the ordinary peek free of the triage decision strip", async () => {
    createAppClient("https://api.test", (async (url: string) =>
      isAccess(url)
        ? response(url, { id: "923e4567-e89b-12d3-a456-426614174009", role: 20 })
        : response(url, issue())) as never);
    renderHost(selection());
    await waitFor(() =>
      expect(screen.getByTestId("notifications-detail-pane")).toHaveAttribute("data-detail-variant", "peek")
    );
    expect(screen.queryByTestId("issue-triage-root")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Accept" })).not.toBeInTheDocument();
  });

  it.each([
    ["guest access", { id: "923e4567-e89b-12d3-a456-426614174009", role: ROLE_GUEST }],
    ["missing project access", null],
  ])("keeps an ordinary peek read-only with %s", async (_label, roles) => {
    createAppClient("https://api.test", (async (url: string) =>
      isAccess(url) ? response(url, roles) : response(url, issue())) as never);
    renderHost(selection());
    await waitFor(() => expect(screen.getByTestId("issue-detail-root")).toHaveAttribute("data-editable", "false"));
    expect(screen.getByTestId("notifications-detail-pane")).toHaveAttribute("data-detail-variant", "peek");
    expect(screen.getByTestId("notifications-detail-summary")).toHaveTextContent("Ada Lovelace commented: detail note");
  });

  it("renders the actor avatar when the notification carries one", async () => {
    createAppClient("https://api.test", (async (url: string) =>
      isAccess(url)
        ? response(url, { id: "923e4567-e89b-12d3-a456-426614174009", role: 20 })
        : response(url, issue())) as never);
    renderHost(selection({ actorAvatarUrl: "https://cdn.test/ada.png" }));
    const pane = screen.getByTestId("notifications-detail-pane");
    const avatar = await within(pane).findByRole("img", { name: "Ada Lovelace" });
    expect(avatar.querySelector("img")).toHaveAttribute("src", "https://cdn.test/ada.png");
  });

  it("keeps a useful read-only summary when the issue hydrate fails", async () => {
    createAppClient("https://api.test", (async (url: string) =>
      isAccess(url)
        ? response(url, { detail: "no access" }, 404)
        : response(url, { detail: "not found" }, 404)) as never);
    renderHost(selection());
    const pane = screen.getByTestId("notifications-detail-pane");
    await waitFor(() =>
      expect(within(pane).getByTestId("notifications-detail-summary")).toHaveTextContent(
        "Ada Lovelace commented: detail note"
      )
    );
    expect(within(pane).getByText("Ship it")).toBeInTheDocument();
    expect(within(pane).getByText("The work item details could not be loaded.")).toBeInTheDocument();
    expect(within(pane).getByTestId("notifications-detail-close")).toBeInTheDocument();
    expect(screen.queryByTestId("issue-detail-root")).not.toBeInTheDocument();
  });
});
