// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Approvals screen container (NEWFRONT-547, RUN-023/RUN-024). The queue
// read follows the area scope, deciding marks one card pending, and a
// refused decision toasts while the card stays.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ApprovalRequest } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const notifyError = vi.fn();
vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifyError: (title: string, description?: string) => notifyError(title, description) };
});
vi.mock("../area.js", () => ({
  useRunnersArea: () => ({
    scope: { workspaceSlug: "acme", projectId: "p-1" },
    workspaceId: "ws-id",
    access: "allowed",
    allowed: true,
    base: "/acme/projects/p-1/runners",
    path: (child?: string) =>
      child === undefined ? "/acme/projects/p-1/runners" : `/acme/projects/p-1/runners/${child}`,
  }),
}));
const queryMocks = vi.hoisted(() => ({
  useApprovals: vi.fn(),
  useDecideApproval: vi.fn(),
}));
vi.mock("../api/approvals.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../api/approvals.js")>();
  return { ...original, ...queryMocks };
});

const { APPROVAL_DECIDE_FAILED_TITLE, ApprovalsScreen } = await import("./ApprovalsScreen.js");

const approval = (overrides: Partial<ApprovalRequest> = {}): ApprovalRequest =>
  ({
    id: "ap-1",
    agent_run: "run-9",
    kind: "file_change",
    payload: { path: "/tmp/x" },
    reason: "sweeping",
    status: "pending",
    decision_source: "",
    requested_at: "2026-10-22T10:00:00.000Z",
    decided_at: null,
    expires_at: null,
    ...overrides,
  }) as ApprovalRequest;

function renderScreen(): void {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <ApprovalsScreen />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  notifyError.mockClear();
  queryMocks.useApprovals.mockReturnValue({ data: [approval()], isPending: false });
  queryMocks.useDecideApproval.mockReturnValue({ mutateAsync: vi.fn(async () => ({})), isPending: false });
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("ApprovalsScreen", () => {
  it("reads the queue for the area scope", () => {
    renderScreen();
    expect(queryMocks.useApprovals).toHaveBeenCalledWith("acme", "p-1");
    expect(queryMocks.useDecideApproval).toHaveBeenCalledWith("acme");
    expect(screen.getByTestId("approval-card")).toBeInTheDocument();
  });

  it("marks the acted card pending until the decision lands", async () => {
    let release!: () => void;
    const gate = new Promise<unknown>((resolve) => {
      release = () => resolve({});
    });
    queryMocks.useApprovals.mockReturnValue({
      data: [approval({ id: "ap-1" }), approval({ id: "ap-2", payload: { path: "/tmp/y" } })],
      isPending: false,
    });
    const mutateAsync = vi.fn(() => gate);
    queryMocks.useDecideApproval.mockReturnValue({ mutateAsync, isPending: false });
    renderScreen();

    fireEvent.click(screen.getAllByTestId("approval-allow-once")[1] as HTMLElement);
    await waitFor(() => expect(mutateAsync).toHaveBeenCalledWith({ approvalId: "ap-2", decision: "accept" }));
    const denies = screen.getAllByTestId("approval-deny");
    expect(denies[0]).not.toBeDisabled();
    expect(denies[1]).toBeDisabled();
    release();
    await waitFor(() => expect(screen.getAllByTestId("approval-deny")[1]).not.toBeDisabled());
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("toasts and keeps the card when the decision fails", async () => {
    const mutateAsync = vi.fn(async () => {
      throw new Error("nope");
    });
    queryMocks.useDecideApproval.mockReturnValue({ mutateAsync, isPending: false });
    renderScreen();

    fireEvent.click(screen.getByTestId("approval-deny"));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(APPROVAL_DECIDE_FAILED_TITLE, expect.any(String)));
    expect(screen.getByTestId("approval-card")).toBeInTheDocument();
    expect(screen.getByTestId("approval-deny")).not.toBeDisabled();
  });
});
