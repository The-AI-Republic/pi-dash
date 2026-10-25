// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { FULL_SCREEN_PORTAL_ID } from "./document.js";
import { focusableWithin, PeekPanel, trapTabTarget } from "./PeekPanel.js";

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

  it("skips the title block for loading and error bodies", () => {
    setup({ hideTitle: true, children: <p>Loading body</p> });
    expect(screen.queryByRole("textbox", { name: "Work item title" })).not.toBeInTheDocument();
    expect(screen.queryByRole("heading")).not.toBeInTheDocument();
    expect(screen.getByText("Loading body")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Close preview" })).toBeInTheDocument();
  });

  it("traps focus when the caller opts in and leaves the docked peek alone", () => {
    const plain = setup();
    expect(screen.getByTestId("issue-preview")).not.toHaveAttribute("tabindex");
    plain.unmount();
    setup({ trapFocus: true, role: "dialog", label: "Public issue" });
    const panel = screen.getByTestId("issue-preview");
    expect(panel).toHaveAttribute("tabindex", "-1");
    expect(panel).toHaveAttribute("aria-modal", "true");
    expect(panel).toContainElement(document.activeElement as HTMLElement);
  });

  it("wraps Tab and keeps Shift+Tab inside a trapping panel", () => {
    setup({ trapFocus: true, role: "dialog", children: <button type="button">Action</button> });
    const panel = screen.getByTestId("issue-preview");
    const buttons = screen.getAllByRole("button");
    buttons[buttons.length - 1]?.focus();
    fireEvent.keyDown(document, { key: "Tab" });
    expect(document.activeElement).toBe(buttons[0]);
    fireEvent.keyDown(document, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(buttons[buttons.length - 1]);
    fireEvent.keyDown(document, { key: "Tab" });
    expect(document.activeElement).toBe(buttons[0]);
    (document.activeElement as HTMLElement).blur();
    panel.focus();
    expect(document.activeElement).toBe(panel);
  });

  it("centers the modal layout behind a backdrop", () => {
    setup({ layout: "modal" });
    const panel = screen.getByTestId("issue-preview");
    expect(panel).toHaveAttribute("data-peek-layout", "modal");
    expect(panel.className).toContain("max-h-[92vh]");
    expect(screen.getByTestId("peek-backdrop")).toBeInTheDocument();
  });

  it("fills the viewport in the full layout", () => {
    setup({ layout: "full" });
    const panel = screen.getByTestId("issue-preview");
    expect(panel).toHaveAttribute("data-peek-layout", "full");
    expect(panel.className).toContain("inset-0");
    expect(screen.queryByTestId("peek-backdrop")).not.toBeInTheDocument();
  });

  it("paints a stacked panel above its parent", () => {
    setup({ stacked: true });
    expect(screen.getByTestId("issue-preview").className).toContain("z-50");
  });

  it("paints a stacked modal above its parent", () => {
    setup({ layout: "modal", stacked: true });
    expect(screen.getByTestId("issue-preview").className).toContain("z-[60]");
    expect(screen.getByTestId("peek-backdrop").className).toContain("z-[55]");
  });

  it("paints a stacked full panel above its parent", () => {
    setup({ layout: "full", stacked: true });
    expect(screen.getByTestId("issue-preview").className).toContain("z-[60]");
  });

  it("stands Escape down while suppressed", () => {
    const { onClose } = setup({ suppressEscape: true });
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).not.toHaveBeenCalled();
  });

  it("stands Escape down while an input holds focus", async () => {
    const user = userEvent.setup();
    const { onClose } = setup();
    await user.click(screen.getByRole("textbox", { name: "Work item title" }));
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).not.toHaveBeenCalled();
  });

  it("stands Escape down while a dialog is open", () => {
    const { onClose } = setup();
    const dialog = document.createElement("div");
    dialog.setAttribute("role", "dialog");
    document.body.appendChild(dialog);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).not.toHaveBeenCalled();
    dialog.remove();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("reverts the title draft on Escape instead of closing", async () => {
    const user = userEvent.setup();
    const { onClose, onCommitTitle } = setup();
    const field = screen.getByRole("textbox", { name: "Work item title" });
    await user.clear(field);
    await user.type(field, "Abandoned draft");
    fireEvent.keyDown(field, { key: "Escape" });
    expect(field).toHaveValue("Parity first issue");
    expect(onCommitTitle).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
    expect(document.activeElement).not.toBe(field);
  });

  it("closes on outside presses and ignores presses inside", () => {
    const onOutsideClick = vi.fn();
    setup({ onOutsideClick, children: <button type="button">Inside</button> });
    fireEvent.pointerDown(screen.getByRole("button", { name: "Inside" }));
    expect(onOutsideClick).not.toHaveBeenCalled();
    fireEvent.pointerDown(document.body);
    expect(onOutsideClick).toHaveBeenCalledTimes(1);
  });

  it("ignores outside presses without a handler", () => {
    const { onClose } = setup();
    fireEvent.pointerDown(document.body);
    expect(onClose).not.toHaveBeenCalled();
  });

  it("stands outside presses down while a menu is open", () => {
    const onOutsideClick = vi.fn();
    setup({ onOutsideClick });
    const menu = document.createElement("div");
    menu.setAttribute("role", "menu");
    const item = document.createElement("button");
    item.textContent = "Delete";
    menu.appendChild(item);
    document.body.appendChild(menu);
    fireEvent.pointerDown(item);
    expect(onOutsideClick).not.toHaveBeenCalled();
    fireEvent.pointerDown(document.body);
    expect(onOutsideClick).not.toHaveBeenCalled();
    menu.remove();
    fireEvent.pointerDown(document.body);
    expect(onOutsideClick).toHaveBeenCalledTimes(1);
  });

  it("ignores presses on the toast stack", () => {
    const onOutsideClick = vi.fn();
    setup({ onOutsideClick });
    const stack = document.createElement("div");
    stack.setAttribute("data-testid", "toast-stack");
    const toast = document.createElement("div");
    toast.textContent = "Copied";
    stack.appendChild(toast);
    document.body.appendChild(stack);
    fireEvent.pointerDown(toast);
    expect(onOutsideClick).not.toHaveBeenCalled();
    stack.remove();
  });

  it("closes on presses against the document node itself", () => {
    const onOutsideClick = vi.fn();
    setup({ onOutsideClick });
    fireEvent.pointerDown(document);
    expect(onOutsideClick).toHaveBeenCalledTimes(1);
  });

  it("drops the outside listener on unmount", () => {
    const onOutsideClick = vi.fn();
    const { unmount } = setup({ onOutsideClick });
    unmount();
    fireEvent.pointerDown(document.body);
    expect(onOutsideClick).not.toHaveBeenCalled();
  });
});

describe("focus trap helpers", () => {
  it("reads tabbables in document order and skips hidden subtrees", () => {
    const root = document.createElement("div");
    root.innerHTML =
      '<button id="a">A</button><div hidden><button id="b">B</button></div><a id="c" href="#x">C</a><button id="d" disabled>D</button>';
    document.body.appendChild(root);
    expect(focusableWithin(root).map((node) => node.id)).toEqual(["a", "c"]);
    expect(focusableWithin(null)).toEqual([]);
    root.remove();
  });

  it("wraps forward, backward and from outside the cycle", () => {
    const root = document.createElement("div");
    root.innerHTML = '<button id="a">A</button><button id="b">B</button>';
    document.body.appendChild(root);
    const [first, last] = focusableWithin(root);
    if (first === undefined || last === undefined) throw new Error("missing controls");
    expect(trapTabTarget(root, last, false)).toBe(first);
    expect(trapTabTarget(root, first, true)).toBe(last);
    expect(trapTabTarget(root, null, true)).toBe(last);
    expect(trapTabTarget(root, document.body, false)).toBeNull();
    expect(trapTabTarget(root, first, false)).toBeNull();
    root.innerHTML = "";
    expect(trapTabTarget(root, null, false)).toBe("self");
    root.remove();
  });
});

describe("peek guards", () => {
  it("blocks Escape on editable focus and open pickers", async () => {
    const { peekEscapeBlocked } = await import("./PeekPanel.js");
    const { unmount } = setup();
    expect(peekEscapeBlocked()).toBe(false);
    const editor = document.createElement("div");
    // jsdom never flips isContentEditable itself; pin the editable bit
    // the way a mounted rich editor carries it.
    Object.defineProperty(editor, "isContentEditable", { value: true });
    editor.setAttribute("tabindex", "-1");
    document.body.appendChild(editor);
    editor.focus();
    expect(peekEscapeBlocked()).toBe(true);
    editor.remove();
    const picker = document.createElement("div");
    picker.setAttribute("role", "listbox");
    document.body.appendChild(picker);
    expect(peekEscapeBlocked()).toBe(true);
    picker.remove();
    expect(peekEscapeBlocked()).toBe(false);
    unmount();
  });

  it("stands the Escape guard down when nothing holds focus", async () => {
    const { peekEscapeBlocked } = await import("./PeekPanel.js");
    Object.defineProperty(document, "activeElement", { value: null, configurable: true });
    try {
      expect(peekEscapeBlocked()).toBe(false);
    } finally {
      delete (document as unknown as Record<string, unknown>).activeElement;
    }
  });

  it("ignores the caller's own dialog role while a picker inside it still guards", async () => {
    const { peekEscapeBlocked, peekOutsideBlocked } = await import("./PeekPanel.js");
    const { unmount } = setup({ role: "dialog" });
    const panel = screen.getByTestId("issue-preview");
    expect(panel).toHaveAttribute("role", "dialog");
    expect(peekEscapeBlocked(panel)).toBe(false);
    expect(peekOutsideBlocked(document.body, panel)).toBe(false);
    const menu = document.createElement("div");
    menu.setAttribute("role", "menu");
    panel.appendChild(menu);
    expect(peekEscapeBlocked(panel)).toBe(true);
    menu.remove();
    expect(peekEscapeBlocked(panel)).toBe(false);
    unmount();
  });

  it("reports non-elements and selects as they are", async () => {
    const { peekEscapeBlocked, peekOutsideBlocked } = await import("./PeekPanel.js");
    expect(peekOutsideBlocked(null)).toBe(false);
    const select = document.createElement("select");
    document.body.appendChild(select);
    select.focus();
    expect(peekEscapeBlocked()).toBe(true);
    select.remove();
  });
});
