// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The expandable modules search (MOD-004): one icon while closed, opening
// moves focus into the field, typing reports the query live, the first
// Escape clears and the second collapses, and a press outside collapses
// only an empty field.

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { ModulesSearch } from "./ModulesSearch.js";

function renderSearch(props: { query?: string; onQueryChange?: (query: string) => void } = {}) {
  const onQueryChange = props.onQueryChange ?? vi.fn();
  const view = render(<ModulesSearch query={props.query ?? ""} onQueryChange={onQueryChange} />);
  return { ...view, onQueryChange };
}

describe("ModulesSearch", () => {
  it("starts collapsed as an icon-only trigger and focuses the field on open", () => {
    renderSearch();
    expect(screen.queryByTestId("modules-search-field")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Search modules" }));
    expect(screen.getByTestId("modules-search-field")).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Search modules by name" })).toHaveFocus();
  });

  it("reports every keystroke as the query", () => {
    const onChange = vi.fn();
    renderSearch({ onQueryChange: onChange });
    fireEvent.click(screen.getByRole("button", { name: "Search modules" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Search modules by name" }), {
      target: { value: "Alpha" },
    });
    expect(onChange).toHaveBeenCalledWith("Alpha");
  });

  it("opens a field already holding a restored query", () => {
    renderSearch({ query: "Bravo" });
    expect(screen.getByTestId("modules-search-field")).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Search modules by name" })).toHaveValue("Bravo");
  });

  it("clears a filled field on the first Escape and keeps it open", () => {
    const onChange = vi.fn();
    renderSearch({ query: "Alpha", onQueryChange: onChange });
    const field = screen.getByRole("textbox", { name: "Search modules by name" });
    fireEvent.keyDown(field, { key: "Escape" });
    expect(onChange).toHaveBeenCalledWith("");
    expect(screen.getByTestId("modules-search-field")).toBeInTheDocument();
    expect(field).toHaveFocus();
  });

  it("collapses an empty field on Escape and returns focus to the trigger", () => {
    renderSearch();
    fireEvent.click(screen.getByRole("button", { name: "Search modules" }));
    fireEvent.keyDown(screen.getByRole("textbox", { name: "Search modules by name" }), { key: "Escape" });
    expect(screen.queryByTestId("modules-search-field")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Search modules" })).toHaveFocus();
  });

  it("ignores other keys", () => {
    const onChange = vi.fn();
    renderSearch({ query: "Alpha", onQueryChange: onChange });
    fireEvent.keyDown(screen.getByRole("textbox", { name: "Search modules by name" }), { key: "Enter" });
    expect(onChange).not.toHaveBeenCalled();
    expect(screen.getByTestId("modules-search-field")).toBeInTheDocument();
  });

  it("collapses an empty field on an outside press but keeps a filled one", () => {
    const { unmount } = renderSearch();
    fireEvent.click(screen.getByRole("button", { name: "Search modules" }));
    fireEvent.pointerDown(document.body);
    expect(screen.queryByTestId("modules-search-field")).not.toBeInTheDocument();
    unmount();

    renderSearch({ query: "Bravo" });
    fireEvent.pointerDown(document.body);
    expect(screen.getByTestId("modules-search-field")).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Search modules by name" })).toHaveValue("Bravo");
  });

  it("keeps a press inside the field from collapsing it", () => {
    renderSearch();
    fireEvent.click(screen.getByRole("button", { name: "Search modules" }));
    fireEvent.pointerDown(screen.getByTestId("modules-search-field"));
    expect(screen.getByTestId("modules-search-field")).toBeInTheDocument();
  });

  it("clears and collapses through the clear control", () => {
    const onChange = vi.fn();
    const { rerender } = renderSearch({ query: "Alpha", onQueryChange: onChange });
    fireEvent.click(screen.getByRole("button", { name: "Clear search" }));
    expect(onChange).toHaveBeenCalledWith("");
    // The query is controlled: the parent writes the cleared value back.
    rerender(<ModulesSearch query="" onQueryChange={onChange} />);
    expect(screen.queryByTestId("modules-search-field")).not.toBeInTheDocument();
  });
});
