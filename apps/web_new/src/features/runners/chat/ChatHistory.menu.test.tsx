// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Managing entries in the history column (NEWFRONT-589, DESK-009): a row
// menu offers rename and delete only when the surface can write history,
// a streaming conversation says why its actions wait, the names it shows
// are the stored ones, and clear-all is offered only with history.
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { ChatHistory, CHAT_ROW_MENU_LABEL } from "./ChatHistory.js";
import { HISTORY_BUSY_REASON, type ChatHistoryEntry } from "./chatState.js";
import { CLEAR_ACTION, DELETE_ACTION, RENAME_ACTION } from "./historyActions.js";

const entry = (overrides: Partial<ChatHistoryEntry> = {}): ChatHistoryEntry => ({
  id: "s-1",
  title: "Planning",
  subtitle: "1 hour ago",
  active: true,
  busyReason: null,
  ...overrides,
});

async function openRowMenu(title = "Planning"): Promise<void> {
  await userEvent.click(screen.getByRole("button", { name: `${CHAT_ROW_MENU_LABEL}: ${title}` }));
}

describe("ChatHistory management", () => {
  it("offers no row menu and no clear control on a surface that cannot write history", () => {
    render(<ChatHistory entries={[entry()]} onSelect={vi.fn()} onNewChat={vi.fn()} />);
    expect(screen.queryByTestId("runner-chat-row-menu")).toBeNull();
    expect(screen.queryByTestId("runner-chat-history-clear")).toBeNull();
  });

  it("renames and deletes through the row menu", async () => {
    const onRename = vi.fn();
    const onDelete = vi.fn();
    render(
      <ChatHistory entries={[entry()]} onSelect={vi.fn()} onNewChat={vi.fn()} onRename={onRename} onDelete={onDelete} />
    );
    await openRowMenu();
    await userEvent.click(await screen.findByRole("menuitem", { name: RENAME_ACTION }));
    expect(onRename).toHaveBeenCalledWith(expect.objectContaining({ id: "s-1" }));
    await openRowMenu();
    await userEvent.click(await screen.findByRole("menuitem", { name: DELETE_ACTION }));
    expect(onDelete).toHaveBeenCalledWith(expect.objectContaining({ id: "s-1" }));
  });

  it("offers only the actions the page passed", async () => {
    render(<ChatHistory entries={[entry()]} onSelect={vi.fn()} onNewChat={vi.fn()} onRename={vi.fn()} />);
    await openRowMenu();
    expect(await screen.findByRole("menuitem", { name: RENAME_ACTION })).toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: DELETE_ACTION })).toBeNull();
  });

  it("offers only delete when the page has no rename", async () => {
    render(<ChatHistory entries={[entry()]} onSelect={vi.fn()} onNewChat={vi.fn()} onDelete={vi.fn()} />);
    await openRowMenu();
    expect(await screen.findByRole("menuitem", { name: DELETE_ACTION })).toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: RENAME_ACTION })).toBeNull();
  });

  it("shows no row menu when neither action was passed", () => {
    render(<ChatHistory entries={[entry()]} onSelect={vi.fn()} onNewChat={vi.fn()} onClear={vi.fn()} />);
    expect(screen.queryByTestId("runner-chat-row-menu")).toBeNull();
  });

  it("keeps a streaming conversation selectable but its actions unavailable, and says why", async () => {
    const onRename = vi.fn();
    const onDelete = vi.fn();
    render(
      <ChatHistory
        entries={[entry({ busyReason: HISTORY_BUSY_REASON })]}
        onSelect={vi.fn()}
        onNewChat={vi.fn()}
        onRename={onRename}
        onDelete={onDelete}
      />
    );
    expect(screen.getByText("Planning").closest("button")).toBeEnabled();
    await openRowMenu();
    const rename = await screen.findByRole("menuitem", { name: RENAME_ACTION });
    expect(rename).toHaveAttribute("aria-disabled", "true");
    await userEvent.click(rename);
    expect(onRename).not.toHaveBeenCalled();
  });

  it("clears the account's history from the column heading", async () => {
    const onClear = vi.fn();
    render(<ChatHistory entries={[entry()]} onSelect={vi.fn()} onNewChat={vi.fn()} onClear={onClear} />);
    await userEvent.click(screen.getByRole("button", { name: CLEAR_ACTION }));
    expect(onClear).toHaveBeenCalledTimes(1);
  });

  it("keeps clear-all inert with nothing stored", () => {
    render(<ChatHistory entries={[]} onSelect={vi.fn()} onNewChat={vi.fn()} onClear={vi.fn()} />);
    expect(screen.getByRole("button", { name: CLEAR_ACTION })).toBeDisabled();
  });

  it("keeps the entry's own accessible name on the row control", () => {
    render(
      <ChatHistory
        entries={[entry({ id: "s-2", title: "Design" })]}
        onSelect={vi.fn()}
        onNewChat={vi.fn()}
        onRename={vi.fn()}
      />
    );
    const nav = screen.getByRole("navigation", { name: "Previous conversations" });
    expect(within(nav).getByRole("button", { name: `${CHAT_ROW_MENU_LABEL}: Design` })).toBeInTheDocument();
  });
});
