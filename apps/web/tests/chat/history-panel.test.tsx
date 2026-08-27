/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

// Mock the lightweight class-name helper so the test doesn't pull the full
// @pi-dash/utils barrel (which transitively imports @pi-dash/constants).
vi.mock("@pi-dash/utils", () => ({
  cn: (...args: unknown[]) => args.filter(Boolean).join(" "),
}));

import { type ChatHistoryItem, ChatHistoryPanel } from "@/components/chat/history-panel";

const items: ChatHistoryItem[] = [
  { id: "s1", title: "Jul 23, 14:05", subtitle: "2 minutes ago", active: true },
  { id: "s2", title: "Jul 22, 09:30", subtitle: "1 day ago · closed" },
];

function renderPanel(overrides: Partial<Parameters<typeof ChatHistoryPanel>[0]> = {}) {
  const onSelect = vi.fn();
  const onNewChat = vi.fn();
  render(<ChatHistoryPanel heading="Chats" items={items} onSelect={onSelect} onNewChat={onNewChat} {...overrides} />);
  return { onSelect, onNewChat };
}

describe("ChatHistoryPanel", () => {
  it("renders the heading and one entry per item", () => {
    renderPanel();
    expect(screen.getByText("Chats")).toBeTruthy();
    expect(screen.getByText("Jul 23, 14:05")).toBeTruthy();
    expect(screen.getByText("Jul 22, 09:30")).toBeTruthy();
    expect(screen.getByText("1 day ago · closed")).toBeTruthy();
  });

  it("invokes onSelect with the item id when an entry is clicked", () => {
    const { onSelect } = renderPanel();
    fireEvent.click(screen.getByText("Jul 22, 09:30"));
    expect(onSelect).toHaveBeenCalledWith("s2");
  });

  it("invokes onNewChat when the New chat button is clicked", () => {
    const { onNewChat } = renderPanel();
    fireEvent.click(screen.getByText("New chat"));
    expect(onNewChat).toHaveBeenCalledTimes(1);
  });

  it("disables the New chat button while busy", () => {
    const { onNewChat } = renderPanel({ busy: true });
    const button = screen.getByText("New chat").closest("button");
    expect(button?.disabled).toBe(true);
    fireEvent.click(button!);
    expect(onNewChat).not.toHaveBeenCalled();
  });

  it("shows an empty state when there are no items", () => {
    render(
      <ChatHistoryPanel
        heading="Chats"
        items={[]}
        onSelect={vi.fn()}
        onNewChat={vi.fn()}
        emptyState={<div>No chats yet.</div>}
      />
    );
    expect(screen.getByText("No chats yet.")).toBeTruthy();
  });
});
