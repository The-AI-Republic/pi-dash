// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The search box starts collapsed, expands unfocused, filters the
// remembered query as the user types, clears on the first Escape and
// collapses on the second, and restores a remembered query into an
// open box on remount.

import { act, fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { resetCycleSearchStore, useCycleSearchStore } from "../api/searchStore.js";
import { CYCLES_PROJECT_ID } from "../testing.js";
import { CyclesSearchBox } from "./CyclesSearchBox.js";

afterEach(() => {
  resetCycleSearchStore();
});

function renderBox(): void {
  render(<CyclesSearchBox projectId={CYCLES_PROJECT_ID} />);
}

function query(): string {
  return useCycleSearchStore.getState().queryFor(CYCLES_PROJECT_ID);
}

describe("CyclesSearchBox", () => {
  it("starts collapsed as a magnifier and expands unfocused", () => {
    renderBox();
    expect(screen.queryByTestId("cycles-search-box")).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId("cycles-search-toggle"));
    const field = screen.getByPlaceholderText("Search");
    expect(screen.getByTestId("cycles-search-box")).toBeInTheDocument();
    expect(field).not.toHaveFocus();
  });

  it("remembers the typed query in the project store", () => {
    renderBox();
    fireEvent.click(screen.getByTestId("cycles-search-toggle"));
    fireEvent.change(screen.getByPlaceholderText("Search"), { target: { value: "alpha" } });
    expect(query()).toBe("alpha");
  });

  it("clears the text on the first Escape and collapses on the second", () => {
    renderBox();
    fireEvent.click(screen.getByTestId("cycles-search-toggle"));
    const field = screen.getByPlaceholderText("Search");
    fireEvent.change(field, { target: { value: "alpha" } });
    fireEvent.keyDown(field, { key: "Escape" });
    expect(query()).toBe("");
    expect(screen.getByTestId("cycles-search-box")).toBeInTheDocument();
    fireEvent.keyDown(screen.getByPlaceholderText("Search"), { key: "Escape" });
    expect(screen.queryByTestId("cycles-search-box")).not.toBeInTheDocument();
    expect(screen.getByTestId("cycles-search-toggle")).toBeInTheDocument();
  });

  it("ignores other keys", () => {
    renderBox();
    fireEvent.click(screen.getByTestId("cycles-search-toggle"));
    const field = screen.getByPlaceholderText("Search");
    fireEvent.change(field, { target: { value: "alpha" } });
    fireEvent.keyDown(field, { key: "Enter" });
    expect(query()).toBe("alpha");
    expect(screen.getByTestId("cycles-search-box")).toBeInTheDocument();
  });

  it("empties and collapses from the clear control", () => {
    renderBox();
    fireEvent.click(screen.getByTestId("cycles-search-toggle"));
    fireEvent.change(screen.getByPlaceholderText("Search"), { target: { value: "beta" } });
    fireEvent.click(screen.getByRole("button", { name: "Close search" }));
    expect(query()).toBe("");
    expect(screen.queryByTestId("cycles-search-box")).not.toBeInTheDocument();
  });

  it("collapses an empty box on an outside click but keeps a filled one", () => {
    render(
      <div>
        <CyclesSearchBox projectId={CYCLES_PROJECT_ID} />
        <button type="button">away</button>
      </div>
    );
    fireEvent.click(screen.getByTestId("cycles-search-toggle"));
    fireEvent.pointerDown(screen.getByRole("button", { name: "away" }));
    expect(screen.queryByTestId("cycles-search-box")).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId("cycles-search-toggle"));
    fireEvent.change(screen.getByPlaceholderText("Search"), { target: { value: "alpha" } });
    fireEvent.pointerDown(screen.getByRole("button", { name: "away" }));
    expect(screen.getByTestId("cycles-search-box")).toBeInTheDocument();
  });

  it("keeps the box open for clicks inside it", () => {
    renderBox();
    fireEvent.click(screen.getByTestId("cycles-search-toggle"));
    fireEvent.pointerDown(screen.getByPlaceholderText("Search"));
    fireEvent.pointerDown(screen.getByRole("button", { name: "Close search" }));
    expect(screen.getByTestId("cycles-search-box")).toBeInTheDocument();
  });

  it("treats a document-level press as outside and collapses an empty box", () => {
    renderBox();
    fireEvent.click(screen.getByTestId("cycles-search-toggle"));
    act(() => {
      document.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    });
    expect(screen.queryByTestId("cycles-search-box")).not.toBeInTheDocument();
  });

  it("restores a remembered query into an open, unfocused box", () => {
    useCycleSearchStore.getState().setQuery(CYCLES_PROJECT_ID, "alpha");
    renderBox();
    const field = screen.getByPlaceholderText("Search");
    expect(field).toHaveValue("alpha");
    expect(field).not.toHaveFocus();
  });
});
