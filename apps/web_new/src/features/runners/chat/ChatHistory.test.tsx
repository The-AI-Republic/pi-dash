// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Conversation history column (NEWFRONT-549, RUN-026): entries, active
// marker, empty line and the New chat control's busy state.
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { CHAT_HISTORY_EMPTY, CHAT_HISTORY_HEADING, CHAT_NEW_LABEL, ChatHistory } from "./ChatHistory.js";

const entries = [
  { id: "s-2", title: "Oct 18, 10:00 AM", subtitle: "1 hour ago", active: true, busyReason: null },
  { id: "s-1", title: "Oct 17, 09:00 AM", subtitle: "yesterday · closed", active: false, busyReason: null },
];

describe("ChatHistory", () => {
  it("lists entries newest-first with labels and the active marker", async () => {
    render(<ChatHistory entries={entries} onSelect={vi.fn()} onNewChat={vi.fn()} />);
    expect(screen.getByText(CHAT_HISTORY_HEADING)).toBeInTheDocument();
    const buttons = within(screen.getByRole("navigation", { name: "Previous conversations" })).getAllByRole("button");
    expect(buttons).toHaveLength(2);
    expect(buttons[0]).toHaveTextContent("Oct 18, 10:00 AM");
    expect(buttons[0]).toHaveTextContent("1 hour ago");
    expect(buttons[0]).toHaveAttribute("aria-current", "true");
    expect(buttons[1]).toHaveTextContent("yesterday · closed");
    expect(buttons[1]).not.toHaveAttribute("aria-current");
  });

  it("reports the selected entry", async () => {
    const onSelect = vi.fn();
    render(<ChatHistory entries={entries} onSelect={onSelect} onNewChat={vi.fn()} />);
    await userEvent.click(screen.getByText("Oct 17, 09:00 AM"));
    expect(onSelect).toHaveBeenCalledWith("s-1");
  });

  it("starts a new chat and disables the control while busy", async () => {
    const onNewChat = vi.fn();
    const { rerender } = render(<ChatHistory entries={entries} onSelect={vi.fn()} onNewChat={onNewChat} />);
    await userEvent.click(screen.getByRole("button", { name: CHAT_NEW_LABEL }));
    expect(onNewChat).toHaveBeenCalledTimes(1);
    rerender(<ChatHistory entries={entries} onSelect={vi.fn()} onNewChat={onNewChat} busy />);
    const control = screen.getByRole("button", { name: CHAT_NEW_LABEL });
    expect(control).toBeDisabled();
    expect(control).toHaveAttribute("aria-busy", "true");
    await userEvent.click(control);
    expect(onNewChat).toHaveBeenCalledTimes(1);
  });

  it("shows the empty line when there are no conversations", () => {
    render(<ChatHistory entries={[]} onSelect={vi.fn()} onNewChat={vi.fn()} />);
    expect(screen.getByTestId("runner-chat-history-empty")).toHaveTextContent(CHAT_HISTORY_EMPTY);
    expect(screen.queryByRole("button", { name: /Oct/ })).toBeNull();
  });
});
