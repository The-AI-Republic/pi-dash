// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The archived search box: a collapsed magnifier that expands to a
// focused field, live query state, Escape clearing before it
// collapses, the clear control emptying and collapsing at once, and
// outside clicks collapsing only an empty box.

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { resetCycleSearchStore, useCycleSearchStore } from "../../cycles/index.js";
import { archivedCycleStoreSlot } from "../api/archivedCycles.js";
import { ARCHIVED_CYCLES_SEARCH_LABEL, ArchivedCyclesSearchBox } from "./ArchivedCyclesSearchBox.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const SLOT = archivedCycleStoreSlot(PROJECT);

function renderBox(): void {
  render(
    <div>
      <div data-testid="outside">outside</div>
      <ArchivedCyclesSearchBox projectId={PROJECT} />
    </div>
  );
}

afterEach(() => {
  resetCycleSearchStore();
});

describe("ArchivedCyclesSearchBox", () => {
  it("starts collapsed behind the magnifier", () => {
    renderBox();

    expect(screen.getByTestId("archived-cycles-search-toggle")).toHaveAccessibleName(ARCHIVED_CYCLES_SEARCH_LABEL);
    expect(screen.queryByTestId("archived-cycles-search-box")).toBeNull();
  });

  it("expands to a focused field and remembers the typed query in the archived slot", () => {
    renderBox();

    fireEvent.click(screen.getByTestId("archived-cycles-search-toggle"));
    const input = screen.getByTestId("archived-cycles-search-input");
    expect(input).toHaveFocus();
    fireEvent.change(input, { target: { value: "alpha" } });

    expect(useCycleSearchStore.getState().queryFor(SLOT)).toBe("alpha");
    expect(useCycleSearchStore.getState().queryFor(PROJECT)).toBe("");
  });

  it("clears the text on the first Escape and collapses on the second", () => {
    renderBox();
    fireEvent.click(screen.getByTestId("archived-cycles-search-toggle"));
    const input = screen.getByTestId("archived-cycles-search-input");
    fireEvent.change(input, { target: { value: "alpha" } });

    fireEvent.keyDown(input, { key: "Escape" });
    expect(useCycleSearchStore.getState().queryFor(SLOT)).toBe("");
    expect(screen.getByTestId("archived-cycles-search-box")).toBeInTheDocument();
    expect(screen.getByTestId("archived-cycles-search-input")).toHaveFocus();

    fireEvent.keyDown(screen.getByTestId("archived-cycles-search-input"), { key: "Escape" });
    expect(screen.queryByTestId("archived-cycles-search-box")).toBeNull();
    expect(screen.getByTestId("archived-cycles-search-toggle")).toBeInTheDocument();
  });

  it("ignores keys other than Escape", () => {
    renderBox();
    fireEvent.click(screen.getByTestId("archived-cycles-search-toggle"));
    const input = screen.getByTestId("archived-cycles-search-input");
    fireEvent.change(input, { target: { value: "alpha" } });

    fireEvent.keyDown(input, { key: "Enter" });

    expect(useCycleSearchStore.getState().queryFor(SLOT)).toBe("alpha");
    expect(screen.getByTestId("archived-cycles-search-box")).toBeInTheDocument();
  });

  it("empties and collapses through the clear control", () => {
    renderBox();
    fireEvent.click(screen.getByTestId("archived-cycles-search-toggle"));
    fireEvent.change(screen.getByTestId("archived-cycles-search-input"), { target: { value: "alpha" } });

    fireEvent.click(screen.getByTestId("archived-cycles-search-clear"));

    expect(useCycleSearchStore.getState().queryFor(SLOT)).toBe("");
    expect(screen.queryByTestId("archived-cycles-search-box")).toBeNull();
  });

  it("collapses an empty box on outside clicks but keeps a filled one", () => {
    renderBox();
    fireEvent.click(screen.getByTestId("archived-cycles-search-toggle"));

    fireEvent.pointerDown(screen.getByTestId("outside"));
    expect(screen.queryByTestId("archived-cycles-search-box")).toBeNull();

    fireEvent.click(screen.getByTestId("archived-cycles-search-toggle"));
    fireEvent.change(screen.getByTestId("archived-cycles-search-input"), { target: { value: "alpha" } });
    fireEvent.pointerDown(screen.getByTestId("outside"));
    expect(screen.getByTestId("archived-cycles-search-box")).toBeInTheDocument();
  });

  it("ignores clicks inside the box", () => {
    renderBox();
    fireEvent.click(screen.getByTestId("archived-cycles-search-toggle"));

    fireEvent.pointerDown(screen.getByTestId("archived-cycles-search-input"));

    expect(screen.getByTestId("archived-cycles-search-box")).toBeInTheDocument();
  });

  it("reopens with a remembered query", () => {
    useCycleSearchStore.getState().setQuery(SLOT, "alpha");
    renderBox();

    expect(screen.getByTestId("archived-cycles-search-input")).toHaveValue("alpha");
  });
});
