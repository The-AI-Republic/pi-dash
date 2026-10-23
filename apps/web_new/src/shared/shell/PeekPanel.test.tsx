// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { FULL_SCREEN_PORTAL_ID } from "./document.js";
import { PeekPanel } from "./PeekPanel.js";

function setup(props?: Partial<React.ComponentProps<typeof PeekPanel>>) {
  const onClose = vi.fn();
  const onCommitTitle = vi.fn();
  document.body.innerHTML = `<div id="${FULL_SCREEN_PORTAL_ID}"></div>`;
  const view = render(
    <PeekPanel
      title="Parity first issue"
      titleLabel="Work item title"
      onClose={onClose}
      onCommitTitle={onCommitTitle}
      {...props}
    />
  );
  return { onClose, onCommitTitle, ...view };
}

describe("PeekPanel", () => {
  it("mounts the panel with its eyebrow, title field and body in the fullscreen portal", () => {
    setup({ eyebrow: "ABC-12", children: <p>Preview body</p> });
    const panel = screen.getByTestId("issue-preview");
    expect(panel).toHaveAttribute("aria-label", "Work item preview");
    expect(document.getElementById(FULL_SCREEN_PORTAL_ID)).toContainElement(panel);
    expect(screen.getByText("ABC-12")).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Work item title" })).toHaveValue("Parity first issue");
    expect(screen.getByText("Preview body")).toBeInTheDocument();
  });

  it("renders nothing when the portal root is missing", () => {
    document.body.innerHTML = "";
    render(<PeekPanel title="Hidden" titleLabel="Work item title" onClose={vi.fn()} />);
    expect(screen.queryByTestId("issue-preview")).not.toBeInTheDocument();
  });

  it("commits a changed title on blur", async () => {
    const user = userEvent.setup();
    const { onCommitTitle } = setup();
    const field = screen.getByRole("textbox", { name: "Work item title" });
    await user.clear(field);
    await user.type(field, "Renamed issue");
    fireEvent.blur(field);
    expect(onCommitTitle).toHaveBeenCalledWith("Renamed issue");
  });

  it("commits a changed title on Enter", async () => {
    const user = userEvent.setup();
    const { onCommitTitle } = setup();
    const field = screen.getByRole("textbox", { name: "Work item title" });
    await user.clear(field);
    await user.type(field, "Renamed issue{Enter}");
    expect(onCommitTitle).toHaveBeenCalledWith("Renamed issue");
  });

  it("ignores other keys and restores the title on empty commits", async () => {
    const user = userEvent.setup();
    const { onCommitTitle } = setup();
    const field = screen.getByRole("textbox", { name: "Work item title" });
    await user.type(field, "{ArrowLeft}");
    expect(onCommitTitle).not.toHaveBeenCalled();
    await user.clear(field);
    await user.type(field, "   ");
    fireEvent.blur(field);
    expect(onCommitTitle).not.toHaveBeenCalled();
    expect(field).toHaveValue("Parity first issue");
  });

  it("leaves unchanged titles uncommitted", () => {
    const { onCommitTitle } = setup();
    const field = screen.getByRole("textbox", { name: "Work item title" });
    fireEvent.blur(field);
    expect(onCommitTitle).not.toHaveBeenCalled();
  });

  it("keeps edits local without a commit handler", async () => {
    const user = userEvent.setup();
    setup({ onCommitTitle: undefined });
    const field = screen.getByRole("textbox", { name: "Work item title" });
    await user.clear(field);
    await user.type(field, "Local draft");
    fireEvent.blur(field);
    expect(field).toHaveValue("Local draft");
  });

  it("resets the draft when the title prop resolves", () => {
    const { rerender } = setup();
    const field = screen.getByRole("textbox", { name: "Work item title" });
    fireEvent.change(field, { target: { value: "Stale draft" } });
    expect(field).toHaveValue("Stale draft");
    rerender(<PeekPanel title="Server title" titleLabel="Work item title" onClose={vi.fn()} onCommitTitle={vi.fn()} />);
    expect(screen.getByRole("textbox", { name: "Work item title" })).toHaveValue("Server title");
  });

  it("closes from the button and the Escape key", () => {
    const { onClose } = setup();
    fireEvent.click(screen.getByRole("button", { name: "Close preview" }));
    expect(onClose).toHaveBeenCalledTimes(1);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(2);
    fireEvent.keyDown(document, { key: "Enter" });
    expect(onClose).toHaveBeenCalledTimes(2);
  });

  it("drops the Escape listener on unmount", () => {
    const { onClose, unmount } = setup();
    unmount();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).not.toHaveBeenCalled();
  });

  it("names the panel for non-work-item previews", () => {
    setup({ label: "Cycle preview", testId: "cycle-preview" });
    const panel = screen.getByTestId("cycle-preview");
    expect(panel).toHaveAttribute("aria-label", "Cycle preview");
    expect(screen.queryByTestId("issue-preview")).not.toBeInTheDocument();
  });

  it("renders a read-only heading when the title is not editable", () => {
    setup({ title: "Cycle 7", titleAsHeading: true, label: "Cycle preview", testId: "cycle-preview" });
    expect(screen.getByRole("heading", { level: 4, name: "Cycle 7" })).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "Work item title" })).not.toBeInTheDocument();
  });
});
