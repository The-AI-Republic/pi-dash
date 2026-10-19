// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Composer shell (AGT-038/AGT-040/AGT-049): unmodified Enter submits,
// Shift+Enter keeps the newline, blanks never submit, a start locks the
// box, and a missing key locks everything with the reminder line.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { Composer } from "./Composer.js";

function renderComposer(props: Partial<React.ComponentProps<typeof Composer>> = {}) {
  const onSubmit = vi.fn();
  const onDraftChange = vi.fn();
  render(<Composer draft="" onDraftChange={onDraftChange} onSubmit={onSubmit} {...props} />);
  return { onSubmit, onDraftChange };
}

describe("Composer", () => {
  it("submits the trimmed draft with unmodified Enter", () => {
    const { onSubmit } = renderComposer({ draft: "  hello  " });
    fireEvent.keyDown(screen.getByRole("textbox", { name: "Message the assistant" }), { key: "Enter" });
    expect(onSubmit).toHaveBeenCalledWith("hello");
  });

  it("keeps the newline on Shift+Enter and never submits blanks", () => {
    const { onSubmit } = renderComposer({ draft: "line" });
    const box = screen.getByRole("textbox", { name: "Message the assistant" });
    fireEvent.keyDown(box, { key: "Enter", shiftKey: true });
    expect(onSubmit).not.toHaveBeenCalled();
    const { onSubmit: blankSubmit } = renderComposer({ draft: "   " });
    const blankBox = screen.getAllByRole("textbox", { name: "Message the assistant" })[1] as HTMLElement;
    fireEvent.keyDown(blankBox, { key: "Enter" });
    expect(blankSubmit).not.toHaveBeenCalled();
    fireEvent.click(screen.getAllByRole("button", { name: "Send message" })[1] as HTMLElement);
    expect(blankSubmit).not.toHaveBeenCalled();
  });

  it("submits on Shift+Enter's newline but never on a modifier chord", () => {
    const { onSubmit } = renderComposer({ draft: "chord" });
    const box = screen.getByRole("textbox", { name: "Message the assistant" });
    fireEvent.keyDown(box, { key: "Enter", ctrlKey: true });
    fireEvent.keyDown(box, { key: "Enter", metaKey: true });
    fireEvent.keyDown(box, { key: "Enter", altKey: true });
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("submits from the send button", () => {
    const { onSubmit } = renderComposer({ draft: "ship it" });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    expect(onSubmit).toHaveBeenCalledWith("ship it");
  });

  it("locks while a start is in flight", () => {
    const { onSubmit } = renderComposer({ draft: "hello", starting: true });
    const box = screen.getByRole("textbox", { name: "Message the assistant" });
    expect(box).toBeDisabled();
    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
    fireEvent.keyDown(box, { key: "Enter" });
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("locks without a provider key and explains why", () => {
    const { onSubmit } = renderComposer({ draft: "hello", configured: false });
    expect(screen.getByTestId("assistant-composer")).toHaveAttribute("data-locked", "true");
    expect(screen.getByTestId("assistant-composer-reason")).toHaveTextContent(
      "Please set your API key in Settings first to start using AI Assistant."
    );
    const box = screen.getByRole("textbox", { name: "Message the assistant" });
    expect(box).toBeDisabled();
    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
    fireEvent.submit(box.closest("form") as HTMLFormElement);
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("swaps send for stop while a turn runs, and a plain Enter still posts", () => {
    // The old surface keeps the box editable and posts during a turn; the
    // server owns the race and answers `turn_active`. Swallowing Enter here
    // stranded the draft with no way to tell why (AGT-049).
    const onStop = vi.fn();
    const { onSubmit } = renderComposer({ draft: "hello", running: true, onStop });
    const box = screen.getByRole("textbox", { name: "Message the assistant" });
    expect(box).toBeEnabled();
    expect(screen.queryByRole("button", { name: "Send message" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Stop response" }));
    expect(onStop).toHaveBeenCalledTimes(1);
    fireEvent.keyDown(box, { key: "Enter" });
    expect(onSubmit).toHaveBeenCalledWith("hello");
  });

  it("keeps modified Enter and Shift+Enter out of the send path while a turn runs", () => {
    const { onSubmit } = renderComposer({ draft: "hello", running: true });
    const box = screen.getByRole("textbox", { name: "Message the assistant" });
    fireEvent.keyDown(box, { key: "Enter", ctrlKey: true });
    fireEvent.keyDown(box, { key: "Enter", shiftKey: true });
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("never sends a blank draft while a turn runs", () => {
    const { onSubmit } = renderComposer({ draft: "   ", running: true });
    const box = screen.getByRole("textbox", { name: "Message the assistant" });
    fireEvent.keyDown(box, { key: "Enter" });
    fireEvent.submit(box.closest("form") as HTMLFormElement);
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("locks the box and shows the posting state while a send is in flight", () => {
    const { onSubmit } = renderComposer({ draft: "hello", posting: true });
    const box = screen.getByRole("textbox", { name: "Message the assistant" });
    expect(box).toBeDisabled();
    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
    fireEvent.keyDown(box, { key: "Enter" });
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("applies the bordered variant only when docked", () => {
    render(<Composer draft="" onDraftChange={() => {}} onSubmit={() => {}} bordered={false} className="probe" />);
    const form = screen.getByTestId("assistant-composer");
    expect(form).toHaveClass("probe");
    expect(form).not.toHaveClass("border-t");
  });

  it("reports draft edits", () => {
    const { onDraftChange } = renderComposer();
    fireEvent.change(screen.getByRole("textbox", { name: "Message the assistant" }), { target: { value: "typed" } });
    expect(onDraftChange).toHaveBeenCalledWith("typed");
  });
});
