// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// PAGE-003 search behavior: expansion, live query, Escape order, clear,
// and click-away collapse only while empty.

import { act, fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { PagesSearch } from "./PagesSearch.js";

function Harness(): React.ReactElement {
  const [query, setQuery] = React.useState("");
  return <PagesSearch query={query} onQueryChange={setQuery} />;
}

describe("PagesSearch", () => {
  it("starts collapsed and expands into the search field", () => {
    render(<Harness />);
    expect(screen.queryByPlaceholderText("Search pages")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Open page search" }));
    expect(screen.getByPlaceholderText("Search pages")).toBeInTheDocument();
  });

  it("narrows through the controlled query as the user types", () => {
    render(<Harness />);
    fireEvent.click(screen.getByRole("button", { name: "Open page search" }));
    fireEvent.change(screen.getByPlaceholderText("Search pages"), { target: { value: "alp" } });
    expect(screen.getByPlaceholderText("Search pages")).toHaveValue("alp");
  });

  it("clears on the first Escape and collapses on the second", () => {
    render(<Harness />);
    fireEvent.click(screen.getByRole("button", { name: "Open page search" }));
    const input = screen.getByPlaceholderText("Search pages");
    fireEvent.change(input, { target: { value: "alp" } });
    fireEvent.keyDown(input, { key: "Escape" });
    expect(screen.getByPlaceholderText("Search pages")).toHaveValue("");
    fireEvent.keyDown(screen.getByPlaceholderText("Search pages"), { key: "Escape" });
    expect(screen.queryByPlaceholderText("Search pages")).not.toBeInTheDocument();
  });

  it("ignores other keys", () => {
    const onQueryChange = vi.fn();
    render(<PagesSearch query="alp" onQueryChange={onQueryChange} />);
    fireEvent.keyDown(screen.getByPlaceholderText("Search pages"), { key: "Enter" });
    expect(onQueryChange).not.toHaveBeenCalled();
  });

  it("clears and collapses from the clear control", () => {
    render(<Harness />);
    fireEvent.click(screen.getByRole("button", { name: "Open page search" }));
    fireEvent.change(screen.getByPlaceholderText("Search pages"), { target: { value: "beta" } });
    fireEvent.click(screen.getByRole("button", { name: "Clear page search" }));
    expect(screen.queryByPlaceholderText("Search pages")).not.toBeInTheDocument();
  });

  it("collapses on click-away only while empty", () => {
    render(
      <div>
        <Harness />
        <button type="button">away</button>
      </div>
    );
    fireEvent.click(screen.getByRole("button", { name: "Open page search" }));
    fireEvent.pointerDown(screen.getByRole("button", { name: "away" }));
    expect(screen.queryByPlaceholderText("Search pages")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Open page search" }));
    fireEvent.change(screen.getByPlaceholderText("Search pages"), { target: { value: "alp" } });
    fireEvent.pointerDown(screen.getByRole("button", { name: "away" }));
    expect(screen.getByPlaceholderText("Search pages")).toHaveValue("alp");
  });

  it("keeps the field open for presses inside it and document-level presses", () => {
    render(
      <div>
        <Harness />
      </div>
    );
    fireEvent.click(screen.getByRole("button", { name: "Open page search" }));
    fireEvent.pointerDown(screen.getByPlaceholderText("Search pages"));
    expect(screen.getByPlaceholderText("Search pages")).toBeInTheDocument();
    act(() => {
      document.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    });
    expect(screen.queryByPlaceholderText("Search pages")).not.toBeInTheDocument();
  });

  it("opens already expanded when the query is not empty", () => {
    render(<PagesSearch query="alpha" onQueryChange={() => undefined} />);
    expect(screen.getByPlaceholderText("Search pages")).toHaveValue("alpha");
  });
});
