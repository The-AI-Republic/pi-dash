// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Expiring approval requests (NEWFRONT-589, DESK-008): a request with a
// deadline shows how long is left, keeps its controls responsive while the
// clock runs, and stops offering answers once it has run out.
import { act, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { approvalCountdownText, approvalSecondsLeft } from "./ChatApprovalPrompt.js";

describe("approvalSecondsLeft", () => {
  it("has no answer for a request that does not expire or names an unreadable deadline", () => {
    expect(approvalSecondsLeft(null, 0)).toBeNull();
    expect(approvalSecondsLeft("not a date", 0)).toBeNull();
  });

  it("rounds a partial second up so a live request never reads as zero early", () => {
    expect(approvalSecondsLeft(new Date(2_500).toISOString(), 1_000)).toBe(2);
  });

  it("floors at zero rather than going negative", () => {
    expect(approvalSecondsLeft(new Date(1_000).toISOString(), 9_000)).toBe(0);
  });
});

describe("approvalCountdownText", () => {
  it("says the request timed out at zero", () => {
    expect(approvalCountdownText(0)).toBe("This request timed out.");
  });

  it("counts seconds under a minute and minutes past it", () => {
    expect(approvalCountdownText(9)).toBe("Expires in 9s");
    expect(approvalCountdownText(60)).toBe("Expires in 1m");
    expect(approvalCountdownText(90)).toBe("Expires in 1m 30s");
  });
});

describe("approval countdown rendering", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("ticks down while the decision controls stay usable, then runs out", async () => {
    vi.useFakeTimers({ toFake: ["Date", "setInterval", "clearInterval"] });
    vi.setSystemTime(new Date(10_000));
    const { ChatApprovalPrompt } = await import("./ChatApprovalPrompt.js");
    render(
      <ChatApprovalPrompt
        request={{
          localApprovalId: "a-1",
          kind: "command_execution",
          reason: "check",
          payload: { command: "ls" },
          expiresAt: new Date(12_500).toISOString(),
        }}
        onDecide={vi.fn()}
      />
    );
    const timer = screen.getByTestId("chat-approval-countdown");
    expect(timer).toHaveTextContent("Expires in 3s");
    expect(screen.getByRole("button", { name: "Approve" })).toBeEnabled();
    act(() => {
      vi.advanceTimersByTime(2_000);
    });
    expect(timer).toHaveTextContent("Expires in 1s");
    expect(screen.getByRole("button", { name: "Approve" })).toBeEnabled();
    act(() => {
      vi.advanceTimersByTime(1_000);
    });
    expect(timer).toHaveTextContent("This request timed out.");
    for (const button of screen.getAllByRole("button")) expect(button).toBeDisabled();
  });

  it("shows no countdown when the request does not expire", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    const { ChatApprovalPrompt } = await import("./ChatApprovalPrompt.js");
    render(
      <ChatApprovalPrompt
        request={{ localApprovalId: "a-1", kind: "file_change", reason: "", payload: {}, expiresAt: null }}
        onDecide={vi.fn()}
      />
    );
    expect(screen.queryByTestId("chat-approval-countdown")).toBeNull();
  });
});
