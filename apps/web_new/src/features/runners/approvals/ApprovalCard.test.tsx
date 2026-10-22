// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One pending-approval card (NEWFRONT-547, RUN-023/RUN-024). The card
// shows the owning run, request time, kind heading, reason, expiry and
// full payload; the three decision buttons share one loading flag.
import { fireEvent, render, screen } from "@testing-library/react";
import type { ApprovalRequest } from "@pidash/api-client";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { ApprovalCard, approvalClockTime } from "./ApprovalCard.js";

const approval = (overrides: Partial<ApprovalRequest> = {}): ApprovalRequest =>
  ({
    id: "ap-1",
    agent_run: "run-9",
    kind: "command_execution",
    payload: { command: "ls", marker: "m-1" },
    reason: "listing files",
    status: "pending",
    decision_source: "",
    requested_at: "2026-10-22T10:00:00.000Z",
    decided_at: null,
    expires_at: "2026-10-22T11:00:00.000Z",
    ...overrides,
  }) as ApprovalRequest;

describe("approvalClockTime", () => {
  it("renders the local clock time", () => {
    expect(approvalClockTime("2026-10-22T10:00:00.000Z")).toBe(
      new Date("2026-10-22T10:00:00.000Z").toLocaleTimeString()
    );
  });
});

describe("ApprovalCard", () => {
  it("shows run, time, kind, reason, expiry and payload", () => {
    render(<ApprovalCard approval={approval()} pending={false} onDecide={() => undefined} />);
    const card = screen.getByTestId("approval-card");
    expect(card).toHaveAttribute("data-approval-id", "ap-1");
    const header = screen.getByTestId("approval-card-header");
    expect(header.textContent).toContain("run-9");
    expect(header.textContent).toContain(approvalClockTime("2026-10-22T10:00:00.000Z"));
    expect(screen.getByTestId("approval-card-kind")).toHaveTextContent("Run a shell command");
    expect(screen.getByTestId("approval-card-reason")).toHaveTextContent("listing files");
    expect(screen.getByTestId("approval-card-expiry").textContent).toContain(
      approvalClockTime("2026-10-22T11:00:00.000Z")
    );
    expect(screen.getByTestId("approval-card-payload").textContent).toBe(
      JSON.stringify({ command: "ls", marker: "m-1" }, null, 2)
    );
  });

  it("hides the reason and expiry when the request carries none", () => {
    render(
      <ApprovalCard approval={approval({ reason: "", expires_at: null })} pending={false} onDecide={() => undefined} />
    );
    expect(screen.queryByTestId("approval-card-reason")).toBeNull();
    expect(screen.queryByTestId("approval-card-expiry")).toBeNull();
  });

  it("decides once, for the session, or denies", () => {
    const onDecide = vi.fn();
    render(<ApprovalCard approval={approval()} pending={false} onDecide={onDecide} />);
    fireEvent.click(screen.getByTestId("approval-allow-once"));
    fireEvent.click(screen.getByTestId("approval-allow-session"));
    fireEvent.click(screen.getByTestId("approval-deny"));
    expect(onDecide).toHaveBeenNthCalledWith(1, "ap-1", "accept");
    expect(onDecide).toHaveBeenNthCalledWith(2, "ap-1", "accept_for_session");
    expect(onDecide).toHaveBeenNthCalledWith(3, "ap-1", "decline");
  });

  it("loads every button on the acted card while pending", () => {
    render(<ApprovalCard approval={approval()} pending={true} onDecide={() => undefined} />);
    for (const testId of ["approval-allow-once", "approval-allow-session", "approval-deny"]) {
      const button = screen.getByTestId(testId);
      expect(button).toBeDisabled();
      expect(button).toHaveAttribute("aria-busy", "true");
    }
  });

  it("leaves the buttons idle when another card is pending", () => {
    render(<ApprovalCard approval={approval()} pending={false} onDecide={() => undefined} />);
    for (const testId of ["approval-allow-once", "approval-allow-session", "approval-deny"]) {
      expect(screen.getByTestId(testId)).not.toBeDisabled();
    }
  });
});
