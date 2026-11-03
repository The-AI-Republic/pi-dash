// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { ProjectViewsSearch, VIEWS_SEARCH_TRIGGER_LABEL } from "./ProjectViewsSearch.js";

function Harness({ initial = "" }: { initial?: string }): React.ReactElement {
  const [query, setQuery] = React.useState(initial);
  return <ProjectViewsSearch query={query} onQueryChange={setQuery} />;
}

function input(): HTMLInputElement {
  return screen.getByRole("textbox");
}

describe("ProjectViewsSearch", () => {
  it("shows only the trigger while collapsed and focuses the field on open", () => {
    render(<Harness />);
    expect(screen.getByRole("button", { name: VIEWS_SEARCH_TRIGGER_LABEL })).toBeInTheDocument();
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: VIEWS_SEARCH_TRIGGER_LABEL }));

    expect(input()).toHaveFocus();
    expect(screen.queryByRole("button", { name: VIEWS_SEARCH_TRIGGER_LABEL })).not.toBeInTheDocument();
  });

  it("reports typed text and stays expanded while it holds a query", () => {
    const onQueryChange = vi.fn();
    render(<ProjectViewsSearch query="tri" onQueryChange={onQueryChange} />);
    expect(input()).toHaveValue("tri");
    fireEvent.change(input(), { target: { value: "triage" } });
    expect(onQueryChange).toHaveBeenCalledWith("triage");
  });

  it("clears a non-empty query on the first Escape and collapses empty on the second", () => {
    const onQueryChange = vi.fn();
    const { rerender } = render(<ProjectViewsSearch query="tri" onQueryChange={onQueryChange} />);
    fireEvent.keyDown(input(), { key: "Escape" });
    expect(onQueryChange).toHaveBeenCalledWith("");
    expect(screen.getByRole("textbox")).toBeInTheDocument();

    rerender(<ProjectViewsSearch query="" onQueryChange={onQueryChange} />);
    fireEvent.keyDown(input(), { key: "Escape" });
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: VIEWS_SEARCH_TRIGGER_LABEL })).toHaveFocus();
  });

  it("ignores other keys", () => {
    const onQueryChange = vi.fn();
    render(<ProjectViewsSearch query="tri" onQueryChange={onQueryChange} />);
    fireEvent.keyDown(input(), { key: "Enter" });
    expect(onQueryChange).not.toHaveBeenCalled();
    expect(input()).toHaveValue("tri");
  });

  it("collapses an empty field on an outside press but keeps a query open", () => {
    const { rerender } = render(<ProjectViewsSearch query="" onQueryChange={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: VIEWS_SEARCH_TRIGGER_LABEL }));
    fireEvent.pointerDown(document.body);
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: VIEWS_SEARCH_TRIGGER_LABEL }));
    rerender(<ProjectViewsSearch query="beta" onQueryChange={vi.fn()} />);
    fireEvent.pointerDown(document.body);
    expect(screen.getByRole("textbox")).toHaveValue("beta");
  });

  it("keeps the field open when the press lands inside the control", () => {
    render(<Harness />);
    fireEvent.click(screen.getByRole("button", { name: VIEWS_SEARCH_TRIGGER_LABEL }));
    fireEvent.pointerDown(input());
    expect(screen.getByRole("textbox")).toBeInTheDocument();
  });

  it("clears and collapses from the clear control", () => {
    function ClearHarness(): React.ReactElement {
      const [query, setQuery] = React.useState("tri");
      return <ProjectViewsSearch query={query} onQueryChange={setQuery} />;
    }
    render(<ClearHarness />);
    fireEvent.click(screen.getByRole("button", { name: "Clear search" }));
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: VIEWS_SEARCH_TRIGGER_LABEL })).toBeInTheDocument();
  });
});
