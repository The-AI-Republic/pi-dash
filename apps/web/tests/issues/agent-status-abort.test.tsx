/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { TIssue } from "@pi-dash/types";

const { abortRun, reTick } = vi.hoisted(() => ({
  abortRun: vi.fn(),
  reTick: vi.fn(),
}));

vi.mock("@pi-dash/constants", () => ({
  API_BASE_URL: "",
}));

vi.mock("@pi-dash/utils", () => ({
  cn: (...args: unknown[]) => args.filter(Boolean).join(" "),
}));

vi.mock("@pi-dash/i18n", () => ({
  useTranslation: () => ({
    t: (key: string, vars?: Record<string, unknown>) =>
      vars ? key.replace(/\{(\w+)\}/g, (_m, k) => String(vars[k] ?? "")) : key,
  }),
}));

vi.mock("@pi-dash/propel/toast", () => ({
  TOAST_TYPE: { SUCCESS: "success", ERROR: "error", INFO: "info" },
  setToast: vi.fn(),
}));

vi.mock("@pi-dash/propel/badge", () => ({
  Badge: ({ children }: { children: React.ReactNode }) => <span>{children}</span>,
}));

vi.mock("@pi-dash/propel/button", () => ({
  Button: ({
    children,
    onClick,
    disabled,
  }: {
    children: React.ReactNode;
    onClick?: () => void;
    disabled?: boolean;
  }) => (
    <button type="button" onClick={onClick} disabled={disabled}>
      {children}
    </button>
  ),
}));

// Surface the confirmation dialog's submit affordance only while open so the
// test can drive the confirm path without the real modal implementation.
vi.mock("@pi-dash/ui", () => ({
  AlertModalCore: ({ isOpen, handleSubmit }: { isOpen: boolean; handleSubmit: () => void }) =>
    isOpen ? (
      <button type="button" onClick={handleSubmit}>
        confirm-abort
      </button>
    ) : null,
}));

// Both use-abort-run and use-re-tick construct an AgentRunService at module
// load; mock the shared service so the hooks call our spies.
vi.mock("@/services/runner", () => ({
  AgentRunService: class {
    abortRun = abortRun;
    reTick = reTick;
  },
}));

import { IssueAgentStatusPanel } from "../../core/components/issues/issue-detail/agent-status";

const ISSUE_OPS = { fetch: vi.fn() };

function makeIssue(status: string): TIssue {
  return {
    id: "issue-1",
    agent_status: {
      ticker: null,
      active_run: {
        id: "run-1",
        status,
        runner: "runner-1",
        runner_name: "workx_claude01",
        created_at: "2026-07-21T00:00:00Z",
        assigned_at: null,
        started_at: null,
        ended_at: null,
        done_payload: null,
        error: "",
        error_diagnostic: null,
        llm_model: "",
        input_tokens: null,
        output_tokens: null,
        total_tokens: null,
      },
      latest_run: null,
      run_count: 1,
    },
  } as unknown as TIssue;
}

function renderPanel(issue: TIssue) {
  return render(
    <IssueAgentStatusPanel
      workspaceSlug="acme"
      projectId="proj-1"
      issueId="issue-1"
      issue={issue}
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      issueOperations={ISSUE_OPS as any}
    />
  );
}

describe("IssueAgentStatusPanel abort run", () => {
  beforeEach(() => {
    abortRun.mockReset();
    reTick.mockReset();
    ISSUE_OPS.fetch.mockReset();
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it("shows the abort button while a run is active", () => {
    renderPanel(makeIssue("running"));
    expect(screen.getByText("Abort run")).toBeTruthy();
  });

  it("confirming abort signals the runner and refreshes the card", async () => {
    abortRun.mockResolvedValue({ id: "run-1", status: "cancelled" });
    renderPanel(makeIssue("running"));

    fireEvent.click(screen.getByText("Abort run"));
    fireEvent.click(screen.getByText("confirm-abort"));

    await waitFor(() => expect(abortRun).toHaveBeenCalledWith("run-1", "user"));
    await waitFor(() => expect(ISSUE_OPS.fetch).toHaveBeenCalledWith("acme", "proj-1", "issue-1"));
  });

  it("hides the abort button once the run is terminal", () => {
    renderPanel(makeIssue("cancelled"));
    expect(screen.queryByText("Abort run")).toBeNull();
  });
});
