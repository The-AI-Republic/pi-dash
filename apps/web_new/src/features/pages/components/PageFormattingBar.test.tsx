// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sticky formatting bar (parity/specs/pages.md §5.3, §9.2,
// PAGE-028–032): the typography selector's label and entries, one icon
// button per control with its tooltip and active highlight, and the media
// slot the next slice appends to.

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { EMPTY_FORMATTING_STATE, type PageFormattingState } from "../../../shared/editor/index.js";
import { PAGE_TOOLBAR_CONTROLS } from "../lib/pageToolbar.js";
import {
  formattingBlockLabel,
  PageFormattingBar,
  PageToolbarHint,
  PAGE_FORMATTING_BAR_LABEL,
  PAGE_TYPOGRAPHY_LABEL,
} from "./PageFormattingBar.js";

function state(overrides: Partial<PageFormattingState> = {}): PageFormattingState {
  return { ...EMPTY_FORMATTING_STATE, ...overrides };
}

function renderBar(props: Partial<React.ComponentProps<typeof PageFormattingBar>> = {}) {
  const onCommand = vi.fn();
  const onBlockKind = vi.fn();
  render(<PageFormattingBar state={state()} onCommand={onCommand} onBlockKind={onBlockKind} {...props} />);
  return { onCommand, onBlockKind };
}

describe("PageFormattingBar controls", () => {
  it("renders one icon button per core control, in reading order", () => {
    renderBar();
    expect(screen.getByRole("toolbar", { name: PAGE_FORMATTING_BAR_LABEL })).toBeInTheDocument();
    const buttons = PAGE_TOOLBAR_CONTROLS.map((control) => screen.getByTestId(`page-toolbar-${control.command}`));
    expect(buttons.map((button) => button.getAttribute("aria-label"))).toEqual(
      PAGE_TOOLBAR_CONTROLS.map((control) => control.label)
    );
  });

  it("fires the matching command on press", () => {
    const { onCommand } = renderBar();
    fireEvent.click(screen.getByTestId("page-toolbar-underline"));
    expect(onCommand).toHaveBeenCalledWith("underline");
    fireEvent.click(screen.getByTestId("page-toolbar-taskList"));
    expect(onCommand).toHaveBeenLastCalledWith("taskList");
  });

  it("keeps the editor selection by refusing the button's mouse-down", () => {
    renderBar();
    expect(fireEvent.mouseDown(screen.getByTestId("page-toolbar-bold"))).toBe(false);
  });

  it("lights the active control only", () => {
    renderBar({ state: state({ bold: true, align: "center", list: "task" }) });
    const pressed = (command: string): string | null =>
      screen.getByTestId(`page-toolbar-${command}`).getAttribute("aria-pressed");
    expect(pressed("bold")).toBe("true");
    expect(screen.getByTestId("page-toolbar-bold")).toHaveAttribute("data-active", "true");
    expect(pressed("italic")).toBe("false");
    expect(pressed("alignCenter")).toBe("true");
    expect(pressed("alignLeft")).toBe("false");
    expect(pressed("taskList")).toBe("true");
    expect(pressed("bulletList")).toBe("false");
  });

  it("draws every command's icon", () => {
    renderBar();
    for (const control of PAGE_TOOLBAR_CONTROLS) {
      expect(screen.getByTestId(`page-toolbar-${control.command}`).querySelector("svg")).not.toBeNull();
    }
  });

  it("renders any media controls the next slice appends", () => {
    renderBar({ mediaControls: <button type="button">Table</button> });
    expect(screen.getByRole("button", { name: "Table" })).toBeInTheDocument();
  });
});

describe("PageToolbarHint", () => {
  it("names the action and draws each shortcut key", () => {
    for (const control of PAGE_TOOLBAR_CONTROLS) {
      const { unmount } = render(<PageToolbarHint control={control} />);
      const hint = screen.getByTestId(`page-toolbar-hint-${control.command}`);
      expect(hint).toHaveTextContent(control.label);
      const caps = Array.from(hint.querySelectorAll("kbd")).map((cap) => cap.textContent);
      expect(caps).toEqual(control.hint === null ? [] : control.hint.split(" + "));
      unmount();
    }
  });

  it("shows only the name for a control without a shortcut", () => {
    const quote = PAGE_TOOLBAR_CONTROLS.find((control) => control.command === "blockquote");
    if (quote === undefined) throw new Error("missing quote control");
    render(<PageToolbarHint control={quote} />);
    expect(screen.getByText("Quote")).toBeInTheDocument();
    expect(screen.getByTestId("page-toolbar-hint-blockquote").querySelector("kbd")).toBeNull();
  });
});

describe("TypographySelector", () => {
  it("shows the block under the caret and opens every entry", () => {
    const { onBlockKind } = renderBar({ state: state({ block: "heading3" }) });
    const trigger = screen.getByTestId("page-toolbar-typography");
    expect(trigger).toHaveAccessibleName(PAGE_TYPOGRAPHY_LABEL);
    expect(trigger).toHaveTextContent("Heading 3");
    fireEvent.click(trigger);
    const items = screen.getAllByRole("menuitem");
    expect(items.map((item) => item.textContent)).toEqual([
      "Text",
      "Heading 1",
      "Heading 2",
      "Heading 3",
      "Heading 4",
      "Heading 5",
      "Heading 6",
    ]);
    fireEvent.click(screen.getByRole("menuitem", { name: "Heading 1" }));
    expect(onBlockKind).toHaveBeenCalledWith("heading1");
  });

  it("returns the block to plain text", () => {
    const { onBlockKind } = renderBar({ state: state({ block: "heading2" }) });
    fireEvent.click(screen.getByTestId("page-toolbar-typography"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Text" }));
    expect(onBlockKind).toHaveBeenCalledWith("paragraph");
  });

  it("labels a block, with Text for a mixed selection", () => {
    expect(formattingBlockLabel("heading5")).toBe("Heading 5");
    expect(formattingBlockLabel(null)).toBe("Text");
  });
});
