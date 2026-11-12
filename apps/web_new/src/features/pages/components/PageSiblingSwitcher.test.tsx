// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sibling switcher (parity/specs/pages.md §4.3, PAGE-024/080): the
// trigger opens a listbox, the search narrows it, Escape closes it and a
// pick reports the chosen page id.

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import {
  PageSiblingSwitcher,
  PAGE_SWITCHER_EMPTY,
  PAGE_SWITCHER_LABEL,
  PAGE_SWITCHER_SEARCH,
} from "./PageSiblingSwitcher.js";

const OPTIONS = [
  { id: "a", name: "Alpha", logoProps: {} },
  { id: "b", name: "Beta", logoProps: {} },
];

/** The visible name of each open option, without its mark glyph. */
function optionNames(): (string | null)[] {
  return screen
    .getAllByRole("option")
    .map((option) => option.querySelector('[data-testid="page-switcher-option-name"]')?.textContent ?? null);
}

function trigger(): HTMLElement {
  return screen.getByRole("button", { name: `${PAGE_SWITCHER_LABEL}, current Alpha` });
}

describe("PageSiblingSwitcher", () => {
  it("opens a listbox with every sibling and no search until opened", () => {
    render(<PageSiblingSwitcher currentName="Alpha" options={OPTIONS} onSelect={vi.fn()} />);
    expect(screen.queryByRole("option")).not.toBeInTheDocument();
    fireEvent.click(trigger());
    expect(trigger()).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByPlaceholderText(PAGE_SWITCHER_SEARCH)).toBeInTheDocument();
    expect(optionNames()).toEqual(["Alpha", "Beta"]);
  });

  it("narrows the options as the search is typed", () => {
    render(<PageSiblingSwitcher currentName="Alpha" options={OPTIONS} onSelect={vi.fn()} />);
    fireEvent.click(trigger());
    fireEvent.change(screen.getByPlaceholderText(PAGE_SWITCHER_SEARCH), { target: { value: "bet" } });
    expect(optionNames()).toEqual(["Beta"]);
    fireEvent.change(screen.getByPlaceholderText(PAGE_SWITCHER_SEARCH), { target: { value: "zzz" } });
    expect(screen.queryAllByRole("option")).toEqual([]);
    expect(screen.getByText(PAGE_SWITCHER_EMPTY)).toBeInTheDocument();
  });

  it("reports the picked page and closes", () => {
    const onSelect = vi.fn();
    render(<PageSiblingSwitcher currentName="Alpha" options={OPTIONS} onSelect={onSelect} />);
    fireEvent.click(trigger());
    fireEvent.click(screen.getByRole("option", { name: "Beta" }));
    expect(onSelect).toHaveBeenCalledWith("b");
    expect(screen.queryByRole("option")).not.toBeInTheDocument();
  });

  it("toggles shut on a second press and on Escape", () => {
    render(<PageSiblingSwitcher currentName="Alpha" options={OPTIONS} onSelect={vi.fn()} />);
    fireEvent.click(trigger());
    fireEvent.click(trigger());
    expect(screen.queryByRole("option")).not.toBeInTheDocument();
    fireEvent.click(trigger());
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("option")).not.toBeInTheDocument();
    expect(trigger()).toHaveAttribute("aria-expanded", "false");
  });

  it("ignores other keys", () => {
    render(<PageSiblingSwitcher currentName="Alpha" options={OPTIONS} onSelect={vi.fn()} />);
    fireEvent.click(trigger());
    fireEvent.keyDown(document, { key: "Enter" });
    expect(screen.queryAllByRole("option")).toHaveLength(2);
  });

  it("closes on an outside press while an inside press keeps it open", () => {
    render(<PageSiblingSwitcher currentName="Alpha" options={OPTIONS} onSelect={vi.fn()} />);
    fireEvent.click(trigger());
    fireEvent.mouseDown(screen.getByRole("listbox"));
    expect(screen.queryAllByRole("option")).toHaveLength(2);
    fireEvent.mouseDown(document.body);
    expect(screen.queryByRole("option")).not.toBeInTheDocument();
  });
});
