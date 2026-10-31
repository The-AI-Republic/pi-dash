// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// History dialogs (NEWFRONT-589, DESK-009): the rename box starts from the
// current name, refuses a blank one in place, submits the trimmed name, and
// neither dialog closes while a write is on its way.
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import {
  CLEAR_HISTORY_TITLE,
  DELETE_CHAT_BODY,
  DELETE_CHAT_TITLE,
  HISTORY_TITLE_REQUIRED,
  clearHistoryBody,
} from "./historyActions.js";
import {
  HistoryRemovalDialog,
  RENAME_CONFIRM_LABEL,
  RENAME_DIALOG_TITLE,
  RenameChatDialog,
} from "./ChatHistoryManage.js";

describe("RenameChatDialog", () => {
  it("starts from the conversation's current name and submits the trimmed value", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    render(<RenameChatDialog target={{ id: "s-1", title: "Planning" }} onCancel={vi.fn()} onSubmit={onSubmit} />);
    const field = await screen.findByRole("textbox", { name: "Name" });
    expect(field).toHaveValue("Planning");
    await user.clear(field);
    await user.type(field, "  Roadmap  ");
    await user.click(screen.getByRole("button", { name: RENAME_CONFIRM_LABEL }));
    expect(onSubmit).toHaveBeenCalledWith("Roadmap");
  });

  it("refuses a blank name without submitting and can recover", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    render(<RenameChatDialog target={{ id: "s-1", title: "Planning" }} onCancel={vi.fn()} onSubmit={onSubmit} />);
    const field = await screen.findByRole("textbox", { name: "Name" });
    await user.clear(field);
    await user.click(screen.getByRole("button", { name: RENAME_CONFIRM_LABEL }));
    expect(await screen.findByRole("alert")).toHaveTextContent(HISTORY_TITLE_REQUIRED);
    expect(onSubmit).not.toHaveBeenCalled();
    await user.type(field, "Backlog");
    await user.keyboard("{Enter}");
    expect(onSubmit).toHaveBeenCalledWith("Backlog");
  });

  it("stays open but inert while a rename is in flight", () => {
    render(<RenameChatDialog target={{ id: "s-1", title: "Planning" }} onCancel={vi.fn()} onSubmit={vi.fn()} busy />);
    expect(screen.getByRole("dialog", { name: RENAME_DIALOG_TITLE })).toBeInTheDocument();
    for (const button of within(screen.getByRole("dialog")).getAllByRole("button")) expect(button).toBeDisabled();
  });

  it("resets the box when another conversation is opened for renaming", async () => {
    const { rerender } = render(
      <RenameChatDialog target={{ id: "s-1", title: "Planning" }} onCancel={vi.fn()} onSubmit={vi.fn()} />
    );
    expect(await screen.findByRole("textbox", { name: "Name" })).toHaveValue("Planning");
    rerender(<RenameChatDialog target={{ id: "s-2", title: "Design" }} onCancel={vi.fn()} onSubmit={vi.fn()} />);
    await waitFor(() => expect(screen.getByRole("textbox", { name: "Name" })).toHaveValue("Design"));
  });

  it("closes on Escape when idle and refuses to while a write is running", async () => {
    const user = userEvent.setup();
    const onCancel = vi.fn();
    const { rerender } = render(
      <RenameChatDialog target={{ id: "s-1", title: "Planning" }} onCancel={onCancel} onSubmit={vi.fn()} />
    );
    await screen.findByRole("dialog", { name: RENAME_DIALOG_TITLE });
    await user.keyboard("{Escape}");
    expect(onCancel).toHaveBeenCalledTimes(1);
    rerender(
      <RenameChatDialog target={{ id: "s-1", title: "Planning" }} onCancel={onCancel} onSubmit={vi.fn()} busy />
    );
    await user.keyboard("{Escape}");
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it("cancels through the Cancel button", async () => {
    const user = userEvent.setup();
    const onCancel = vi.fn();
    render(<RenameChatDialog target={{ id: "s-1", title: "Planning" }} onCancel={onCancel} onSubmit={vi.fn()} />);
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onCancel).toHaveBeenCalledTimes(1);
  });
});

describe("HistoryRemovalDialog", () => {
  it("asks about one conversation with its own copy", () => {
    render(
      <HistoryRemovalDialog choice={{ scope: "single" }} sessionCount={2} onCancel={vi.fn()} onConfirm={vi.fn()} />
    );
    expect(screen.getByRole("dialog", { name: DELETE_CHAT_TITLE })).toHaveTextContent(DELETE_CHAT_BODY);
    expect(screen.getByRole("button", { name: "Delete chat" })).toBeEnabled();
  });

  it("counts what clearing removes for this account", () => {
    render(<HistoryRemovalDialog choice={{ scope: "all" }} sessionCount={4} onCancel={vi.fn()} onConfirm={vi.fn()} />);
    expect(screen.getByRole("dialog", { name: CLEAR_HISTORY_TITLE })).toHaveTextContent(clearHistoryBody(4));
  });

  it("confirms once and ignores the button while busy", async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn();
    const { rerender } = render(
      <HistoryRemovalDialog choice={{ scope: "all" }} sessionCount={1} onCancel={vi.fn()} onConfirm={onConfirm} />
    );
    await user.click(screen.getByRole("button", { name: "Clear history" }));
    expect(onConfirm).toHaveBeenCalledTimes(1);
    rerender(
      <HistoryRemovalDialog choice={{ scope: "all" }} sessionCount={1} onCancel={vi.fn()} onConfirm={onConfirm} busy />
    );
    for (const button of within(screen.getByRole("dialog")).getAllByRole("button")) expect(button).toBeDisabled();
  });

  it("closes on Escape when idle and refuses to while a write is running", async () => {
    const user = userEvent.setup();
    const onCancel = vi.fn();
    const { rerender } = render(
      <HistoryRemovalDialog choice={{ scope: "single" }} sessionCount={2} onCancel={onCancel} onConfirm={vi.fn()} />
    );
    await screen.findByRole("dialog", { name: DELETE_CHAT_TITLE });
    await user.keyboard("{Escape}");
    expect(onCancel).toHaveBeenCalledTimes(1);
    rerender(
      <HistoryRemovalDialog
        choice={{ scope: "single" }}
        sessionCount={2}
        onCancel={onCancel}
        onConfirm={vi.fn()}
        busy
      />
    );
    await user.keyboard("{Escape}");
    expect(onCancel).toHaveBeenCalledTimes(1);
  });
});
