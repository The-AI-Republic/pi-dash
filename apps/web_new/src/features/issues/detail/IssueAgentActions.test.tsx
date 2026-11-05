// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The issue-surface agent controls (RUN-046/RUN-048): the row uses the
// runners public dispatch interface, obeys the server re-tick flag, and
// invalidates the issue detail without touching executor selection.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { AgentRun, AgentRunQueued, Issue, ReTickResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const harness = vi.hoisted(() => ({
  triggerRun: vi.fn(),
  reTick: vi.fn(),
  refreshCallbacks: [] as Array<(() => Promise<unknown>) | undefined>,
}));

vi.mock("../../runners/index.js", () => ({
  useTriggerAgentRun: (options: { onSettledRefresh?: () => Promise<unknown> }) => {
    harness.refreshCallbacks.push(options.onSettledRefresh);
    return { triggerRun: harness.triggerRun, isSubmitting: false };
  },
  useReTick: (options: { onSettledRefresh?: () => Promise<unknown> }) => {
    harness.refreshCallbacks.push(options.onSettledRefresh);
    return { reTick: harness.reTick, isSubmitting: false };
  },
}));

const { IssueAgentActions, IssueReTickAction, IssueRunAIAction, reTickOffered } =
  await import("./IssueAgentActions.js");
const { issueKeys } = await import("../api/keys.js");

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";
const run = { id: "run-1", status: "queued" } as AgentRun;
const queued = { queued: true, reason: "active", detail: "one already runs" } as AgentRunQueued;
const granted: ReTickResponse = { granted: true, reason: "exhausted", run_id: "run-2" };

function issueFixture(overrides: Partial<Issue> = {}): Issue {
  return {
    id: ISSUE,
    name: "Fix login",
    state_id: null,
    sort_order: 1,
    completed_at: null,
    estimate_point: null,
    priority: "high",
    sequence_id: 42,
    project_id: PROJECT,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    complexity_score: 0,
    agent_executor: null,
    is_synced: false,
    description_html: "<p>Words</p>",
    is_subscribed: false,
    agent_ticker: null,
    agent_status: null,
    relations_summary: { blocked_by: [], blocking: [] },
    has_open_blockers: false,
    ...overrides,
  };
}

function ticker(canReTick: boolean) {
  return {
    enabled: true,
    user_disabled: false,
    used: 2,
    tick_count: 2,
    granted: 2,
    waited: 0,
    max_ticks: 2,
    remaining: 0,
    interval_seconds: 3600,
    next_run_at: null,
    last_tick_at: null,
    disarm_reason: "",
    pending_entry: false,
    can_re_tick: canReTick,
  };
}

function renderActions(issue: Issue = issueFixture(), disabled = false) {
  const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
  const invalidate = vi.spyOn(queryClient, "invalidateQueries");
  render(
    <QueryClientProvider client={queryClient}>
      <IssueAgentActions
        workspaceSlug={SLUG}
        workspaceId="ws-id"
        projectId={PROJECT}
        issue={issue}
        disabled={disabled}
      />
    </QueryClientProvider>
  );
  return { invalidate };
}

beforeEach(() => {
  harness.refreshCallbacks = [];
  harness.triggerRun.mockResolvedValue(run);
  harness.reTick.mockResolvedValue(granted);
});

afterEach(() => {
  cleanup();
  harness.triggerRun.mockReset();
  harness.reTick.mockReset();
});

describe("reTickOffered", () => {
  it("reads the flag from the status ticker first and the detail fallback second", () => {
    expect(reTickOffered(issueFixture())).toBe(false);
    expect(reTickOffered(issueFixture({ agent_ticker: ticker(true) }))).toBe(true);
    expect(
      reTickOffered(
        issueFixture({
          agent_ticker: ticker(false),
          agent_status: { ticker: ticker(true), active_run: null, latest_run: null, run_count: 0 },
        })
      )
    ).toBe(true);
  });

  it("does not infer a refill from budget counters without the server flag", () => {
    expect(reTickOffered(issueFixture({ agent_ticker: { ...ticker(true), can_re_tick: false } }))).toBe(false);
  });
});

describe("IssueAgentActions", () => {
  it("starts a run with the issue's server ids", async () => {
    const user = userEvent.setup();
    renderActions();
    await user.click(screen.getByTestId("issue-run-ai"));
    expect(harness.triggerRun).toHaveBeenCalledWith({
      workspaceId: "ws-id",
      workspaceSlug: SLUG,
      projectId: PROJECT,
      issueId: ISSUE,
      mode: "run_ai",
    });
  });

  it("hands the detail refresh to both dispatch hooks", async () => {
    const { invalidate } = renderActions();
    expect(harness.refreshCallbacks).toHaveLength(2);
    await harness.refreshCallbacks[0]?.();
    expect(invalidate).toHaveBeenCalledWith({ queryKey: issueKeys.detail(SLUG, ISSUE) });
  });

  it("keeps the action row inert before the workspace id resolves", async () => {
    const user = userEvent.setup();
    const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <IssueAgentActions
          workspaceSlug={SLUG}
          workspaceId={null}
          projectId={PROJECT}
          issue={issueFixture()}
          disabled={false}
        />
      </QueryClientProvider>
    );
    const button = screen.getByTestId("issue-run-ai");
    expect(button).toBeDisabled();
    await user.click(button);
    expect(harness.triggerRun).not.toHaveBeenCalled();
  });

  it("keeps re-tick inert before the workspace id resolves", async () => {
    const user = userEvent.setup();
    const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <IssueAgentActions
          workspaceSlug={SLUG}
          workspaceId={null}
          projectId={PROJECT}
          issue={issueFixture({ agent_ticker: ticker(true) })}
          disabled={false}
        />
      </QueryClientProvider>
    );
    const refill = screen.getByTestId("issue-re-tick");
    expect(refill).toBeDisabled();
    await user.click(refill);
    expect(harness.reTick).not.toHaveBeenCalled();
  });

  it("renders each mount on its own with the workspace id ready", async () => {
    const user = userEvent.setup();
    const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
    const props = {
      workspaceSlug: SLUG,
      workspaceId: "ws-id",
      projectId: PROJECT,
      issue: issueFixture({ agent_ticker: ticker(true) }),
      disabled: false,
    };
    render(
      <QueryClientProvider client={queryClient}>
        <IssueRunAIAction {...props} label="Manually Run AI" size="medium" />
        <IssueReTickAction {...props} />
      </QueryClientProvider>
    );
    const start = screen.getByTestId("issue-run-ai");
    expect(start).toHaveTextContent("Manually Run AI");
    await user.click(start);
    await user.click(screen.getByTestId("issue-re-tick"));
    expect(harness.triggerRun).toHaveBeenCalledTimes(1);
    expect(harness.reTick).toHaveBeenCalledTimes(1);
  });

  it("does not dispatch from a read-only issue surface", async () => {
    const user = userEvent.setup();
    renderActions(issueFixture({ agent_ticker: ticker(true) }), true);
    expect(screen.getByTestId("issue-run-ai")).toBeDisabled();
    expect(screen.getByTestId("issue-re-tick")).toBeDisabled();
    await user.click(screen.getByTestId("issue-run-ai"));
    await user.click(screen.getByTestId("issue-re-tick"));
    expect(harness.triggerRun).not.toHaveBeenCalled();
    expect(harness.reTick).not.toHaveBeenCalled();
  });

  it("renders re-tick only when the server flag says a refill is owed", () => {
    renderActions(issueFixture({ agent_ticker: ticker(false) }));
    expect(screen.queryByTestId("issue-re-tick")).not.toBeInTheDocument();
    cleanup();
    renderActions(issueFixture({ agent_ticker: ticker(true) }));
    expect(screen.getByTestId("issue-re-tick")).toBeInTheDocument();
  });

  it("asks for a refill for this issue", async () => {
    const user = userEvent.setup();
    const { invalidate } = renderActions(issueFixture({ agent_ticker: ticker(true) }));
    await user.click(screen.getByTestId("issue-re-tick"));
    expect(harness.reTick).toHaveBeenCalledWith(ISSUE);
    expect(harness.refreshCallbacks[1]).toBeTypeOf("function");
    await harness.refreshCallbacks[1]?.();
    expect(invalidate).toHaveBeenCalledWith({ queryKey: issueKeys.detail(SLUG, ISSUE) });
  });

  it("does not let a re-tick run while the surface is read-only", async () => {
    const user = userEvent.setup();
    renderActions(issueFixture({ agent_ticker: ticker(true) }), true);
    await user.click(screen.getByTestId("issue-re-tick"));
    expect(harness.reTick).not.toHaveBeenCalled();
  });

  it("keeps both controls available after a queued or failed dispatch", async () => {
    harness.triggerRun.mockResolvedValue(queued);
    const user = userEvent.setup();
    renderActions(issueFixture({ agent_ticker: ticker(true) }));
    await user.click(screen.getByTestId("issue-run-ai"));
    await waitFor(() => expect(harness.triggerRun).toHaveBeenCalled());
    expect(screen.getByTestId("issue-run-ai")).toBeEnabled();
    expect(screen.getByTestId("issue-re-tick")).toBeEnabled();
  });

  it("keeps both controls available after a rejected dispatch", async () => {
    let rejectDispatch!: (reason: unknown) => void;
    const pending = new Promise<never>((_resolve, reject) => {
      rejectDispatch = reject;
    });
    // Handled from creation: the mount fires and forgets, so the test owns
    // the rejection and no unhandled rejection escapes the run.
    void pending.catch(() => {});
    harness.triggerRun.mockReturnValue(pending);
    const user = userEvent.setup();
    renderActions(issueFixture({ agent_ticker: ticker(true) }));
    await user.click(screen.getByTestId("issue-run-ai"));
    rejectDispatch(new Error("dispatch failed"));
    await expect(pending).rejects.toThrow("dispatch failed");
    expect(harness.triggerRun).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("issue-run-ai")).toBeEnabled();
    expect(screen.getByTestId("issue-re-tick")).toBeEnabled();
  });
});
