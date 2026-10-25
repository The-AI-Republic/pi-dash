// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Title surface (parity/specs/pages.md §5.1, PAGE-079): the placeholder
// rides the empty editable node, the field never collapses, the count is
// focus-only, and read-only pages show static text.

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { PageDetailTitle, PAGE_TITLE_MAX_LENGTH, PAGE_TITLE_PLACEHOLDER } from "./PageDetailTitle.js";

function editable(name: string, onRename = vi.fn()): { onRename: ReturnType<typeof vi.fn>; field: HTMLElement } {
  const { container } = render(<PageDetailTitle pageId="page-1" name={name} editable onRename={onRename} />);
  const field = container.querySelector<HTMLElement>('[contenteditable="true"]');
  if (field === null) throw new Error("missing editable title");
  return { onRename, field };
}

describe("PageDetailTitle read-only", () => {
  it("renders the page name as a heading", () => {
    render(<PageDetailTitle pageId="page-1" name="Launch notes" editable={false} />);
    expect(screen.getByRole("heading", { name: "Launch notes" })).toHaveAttribute("id", "page-1-title");
  });

  it("falls back to Untitled for a blank name", () => {
    render(<PageDetailTitle pageId="page-1" name="   " editable={false} />);
    expect(screen.getByRole("heading", { name: "Untitled" })).toBeInTheDocument();
  });
});

describe("PageDetailTitle editable", () => {
  it("keeps a stable title container and marks the empty node", () => {
    const { field } = editable("");
    expect(field.closest("#page-1-title")).not.toBeNull();
    expect(field).toHaveClass("is-empty", "min-h-12");
    expect(field).toHaveAttribute("data-placeholder", PAGE_TITLE_PLACEHOLDER);
    expect(field).toHaveAttribute("data-empty", "true");
  });

  it("shows the count only while focused", () => {
    const { field } = editable("abc");
    expect(screen.queryByText(`3/${PAGE_TITLE_MAX_LENGTH}`)).not.toBeInTheDocument();
    fireEvent.focus(field);
    expect(screen.getByText(`3/${PAGE_TITLE_MAX_LENGTH}`)).toBeInTheDocument();
  });

  it("saves the typed name on blur", () => {
    const { onRename, field } = editable("Old");
    fireEvent.focus(field);
    field.textContent = "New";
    fireEvent.input(field);
    fireEvent.blur(field);
    expect(onRename).toHaveBeenCalledWith("New");
  });

  it("does not save an unchanged name", () => {
    const { onRename, field } = editable("Same");
    fireEvent.focus(field);
    fireEvent.blur(field);
    expect(onRename).not.toHaveBeenCalled();
  });

  it("refuses input past the cap", () => {
    const { onRename, field } = editable("Old");
    fireEvent.focus(field);
    field.textContent = "x".repeat(PAGE_TITLE_MAX_LENGTH + 5);
    fireEvent.input(field);
    expect(field.textContent).toHaveLength(PAGE_TITLE_MAX_LENGTH);
    fireEvent.blur(field);
    expect(onRename).toHaveBeenCalledWith("x".repeat(PAGE_TITLE_MAX_LENGTH));
  });

  it("Enter commits without a newline", () => {
    const { onRename, field } = editable("Old");
    field.focus();
    fireEvent.focus(field);
    field.textContent = "Renamed";
    fireEvent.input(field);
    const event = fireEvent.keyDown(field, { key: "Enter" });
    expect(event).toBe(false);
    expect(onRename).toHaveBeenCalledWith("Renamed");
    expect(field.textContent).not.toContain("\n");
  });

  it("leaves other keys alone", () => {
    const { onRename, field } = editable("Old");
    fireEvent.focus(field);
    expect(fireEvent.keyDown(field, { key: "a" })).toBe(true);
    expect(onRename).not.toHaveBeenCalled();
  });

  it("treats a node without text content as empty", () => {
    const { onRename, field } = editable("Old");
    fireEvent.focus(field);
    field.textContent = "";
    fireEvent.input(field);
    fireEvent.blur(field);
    expect(onRename).toHaveBeenCalledWith("");
  });

  it("treats a null text content as an empty draft", () => {
    const { onRename, field } = editable("Old");
    fireEvent.focus(field);
    Object.defineProperty(field, "textContent", { configurable: true, get: () => null });
    fireEvent.input(field);
    expect(screen.getByText(`0/${PAGE_TITLE_MAX_LENGTH}`)).toBeInTheDocument();
    fireEvent.blur(field);
    expect(onRename).toHaveBeenCalledWith("");
  });

  it("pastes only plain text up to the cap", () => {
    const { field } = editable("Old");
    field.focus();
    fireEvent.focus(field);
    const exec = vi.fn();
    Object.defineProperty(document, "execCommand", { value: exec, configurable: true });
    fireEvent.paste(field, { clipboardData: { getData: () => "y".repeat(PAGE_TITLE_MAX_LENGTH + 3) } });
    expect(exec).toHaveBeenCalledWith("insertText", false, "y".repeat(PAGE_TITLE_MAX_LENGTH));
  });

  it("adopts a server-side rename when not focused", () => {
    const { rerender } = render(<PageDetailTitle pageId="page-1" name="Before" editable onRename={vi.fn()} />);
    const field = () => document.querySelector<HTMLElement>('[contenteditable="true"]');
    expect(field()?.textContent).toBe("Before");
    rerender(<PageDetailTitle pageId="page-1" name="After" editable onRename={vi.fn()} />);
    expect(field()?.textContent).toBe("After");
  });

  it("works without a rename handler", () => {
    render(<PageDetailTitle pageId="page-1" name="Old" editable />);
    const field = screen.getByRole("textbox", { name: "Page title" });
    fireEvent.focus(field);
    field.textContent = "New";
    fireEvent.input(field);
    expect(() => fireEvent.blur(field)).not.toThrow();
  });
});
