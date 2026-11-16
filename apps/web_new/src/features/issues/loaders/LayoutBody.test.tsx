// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { LayoutKind } from "../layouts/kinds.js";
import { LayoutBody } from "./LayoutBody.js";

function renderBody(props: { layout: LayoutKind; totalCount: number; resultCount: number }): void {
  render(
    <LayoutBody
      layout={props.layout}
      totalCount={props.totalCount}
      resultCount={props.resultCount}
      emptyState={<p>Nothing here yet</p>}
      noMatchState={<p>No issues match</p>}
    >
      <p>Layout content</p>
    </LayoutBody>
  );
}

describe("LayoutBody", () => {
  it("routes a zero total to the empty state", () => {
    renderBody({ layout: "list", totalCount: 0, resultCount: 0 });
    expect(screen.getByText("Nothing here yet")).toBeInTheDocument();
    expect(screen.queryByText("Layout content")).not.toBeInTheDocument();
  });

  it("routes zero visible rows to the no-match state", () => {
    renderBody({ layout: "kanban", totalCount: 4, resultCount: 0 });
    expect(screen.getByText("No issues match")).toBeInTheDocument();
    expect(screen.queryByText("Layout content")).not.toBeInTheDocument();
  });

  it("renders the layout once rows are visible", () => {
    renderBody({ layout: "spreadsheet", totalCount: 4, resultCount: 2 });
    expect(screen.getByText("Layout content")).toBeInTheDocument();
    expect(screen.queryByText("Nothing here yet")).not.toBeInTheDocument();
  });

  it("always renders the calendar grid, even with no issues", () => {
    renderBody({ layout: "calendar", totalCount: 0, resultCount: 0 });
    expect(screen.getByText("Layout content")).toBeInTheDocument();
    expect(screen.queryByText("Nothing here yet")).not.toBeInTheDocument();
  });

  it("renders the calendar grid when filters match nothing", () => {
    renderBody({ layout: "calendar", totalCount: 4, resultCount: 0 });
    expect(screen.getByText("Layout content")).toBeInTheDocument();
    expect(screen.queryByText("No issues match")).not.toBeInTheDocument();
  });
});
