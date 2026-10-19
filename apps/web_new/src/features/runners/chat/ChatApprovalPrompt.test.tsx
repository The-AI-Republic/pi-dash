// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Inline approval prompt (NEWFRONT-553, RUN-034): the narrowest target and
// the engine's reason, approve/deny for every kind, the session-wide answer
// only for commands, and no clicks while a decision is in flight.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import {
  ALWAYS_ALLOW_LABEL,
  APPROVE_LABEL,
  APPROVAL_PROMPT_LABEL,
  ChatApprovalPrompt,
  DENY_LABEL,
} from "./ChatApprovalPrompt.js";
import type { PendingApproval } from "./approvalCopy.js";

function request(overrides: Partial<PendingApproval> = {}): PendingApproval {
  return {
    localApprovalId: "a-1",
    kind: "command_execution",
    reason: "tests the change",
    payload: { command: "npm test", cwd: "/repo" },
    expiresAt: null,
    ...overrides,
  };
}

describe("ChatApprovalPrompt", () => {
  it("names the request, its target and the engine's reason", () => {
    render(<ChatApprovalPrompt request={request()} onDecide={vi.fn()} />);
    expect(screen.getByRole("alertdialog", { name: APPROVAL_PROMPT_LABEL })).toBeInTheDocument();
    expect(screen.getByText(/wants to run a command/i)).toBeInTheDocument();
    expect(screen.getByTestId("chat-approval-target")).toHaveTextContent("npm test");
    expect(screen.getByTestId("chat-approval-reason")).toHaveTextContent("tests the change");
  });

  it("omits a missing target and reason", () => {
    render(<ChatApprovalPrompt request={request({ reason: "", payload: {} })} onDecide={vi.fn()} />);
    expect(screen.queryByTestId("chat-approval-target")).toBeNull();
    expect(screen.queryByTestId("chat-approval-reason")).toBeNull();
  });

  it("answers with approve and deny", async () => {
    const user = userEvent.setup();
    const onDecide = vi.fn();
    render(<ChatApprovalPrompt request={request()} onDecide={onDecide} />);
    await user.click(screen.getByRole("button", { name: APPROVE_LABEL }));
    await user.click(screen.getByRole("button", { name: DENY_LABEL }));
    expect(onDecide.mock.calls).toEqual([["accept"], ["decline"]]);
  });

  it("offers the session-wide answer only for a command", async () => {
    const user = userEvent.setup();
    const onDecide = vi.fn();
    const { rerender } = render(<ChatApprovalPrompt request={request()} onDecide={onDecide} />);
    await user.click(screen.getByRole("button", { name: ALWAYS_ALLOW_LABEL }));
    expect(onDecide).toHaveBeenCalledWith("accept_for_session");
    rerender(
      <ChatApprovalPrompt request={request({ kind: "file_change", payload: { path: "/a" } })} onDecide={onDecide} />
    );
    expect(screen.queryByRole("button", { name: ALWAYS_ALLOW_LABEL })).toBeNull();
    expect(screen.getByText("The agent wants to change a file.")).toBeInTheDocument();
  });

  it("keeps every control inert while a decision is in flight", () => {
    render(<ChatApprovalPrompt request={request()} onDecide={vi.fn()} busy />);
    for (const button of screen.getAllByRole("button")) expect(button).toBeDisabled();
  });

  it("shows the network and generic sentences", () => {
    const { rerender } = render(
      <ChatApprovalPrompt
        request={request({ kind: "network_access", payload: { host: "api.test" } })}
        onDecide={vi.fn()}
      />
    );
    expect(screen.getByText("The agent wants to reach the network.")).toBeInTheDocument();
    expect(screen.getByTestId("chat-approval-target")).toHaveTextContent("api.test");
    rerender(<ChatApprovalPrompt request={request({ kind: "other" })} onDecide={vi.fn()} />);
    expect(screen.getByText("The agent wants your permission.")).toBeInTheDocument();
  });
});
