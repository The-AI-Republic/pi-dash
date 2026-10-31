// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { LAYOUT_ORDER } from "../layouts/kinds.js";
import { IssuesHeader } from "./IssuesHeader.js";

describe("IssuesHeader", () => {
  it("renders the title, count and switcher, hidden under md", () => {
    const onSwitch = vi.fn();
    render(<IssuesHeader title="Website" totalCount={3} offered={LAYOUT_ORDER} active="list" onSwitch={onSwitch} />);
    const header = screen.getByTestId("issues-header");
    expect(header).toHaveClass("hidden");
    expect(header).toHaveClass("md:flex");
    expect(screen.getByRole("heading", { name: "Website" })).toBeInTheDocument();
    expect(screen.getByText("3 issues")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Calendar layout" }));
    expect(onSwitch).toHaveBeenCalledWith("calendar");
  });

  it("singularises a single issue", () => {
    render(<IssuesHeader title="Website" totalCount={1} offered={["list"]} active="list" onSwitch={() => {}} />);
    expect(screen.getByText("1 issue")).toBeInTheDocument();
  });

  it("hosts trailing controls from later slices", () => {
    render(
      <IssuesHeader
        title="Website"
        totalCount={0}
        offered={["list"]}
        active="list"
        onSwitch={() => {}}
        actions={<button type="button">Display</button>}
      />
    );
    expect(screen.getByRole("button", { name: "Display" })).toBeInTheDocument();
  });

  it("constrains the title so trailing controls stay inside a narrowed column", () => {
    render(
      <IssuesHeader
        title="A very long cycle name that must truncate instead of pushing the switcher under the sidebar"
        totalCount={12}
        offered={LAYOUT_ORDER}
        active="list"
        onSwitch={() => {}}
        actions={<button type="button">Display</button>}
      />
    );
    expect(screen.getByTestId("issues-header")).toHaveClass("min-w-0");
    const titleRegion = screen.getByTestId("issues-header-title");
    expect(titleRegion).toHaveClass("min-w-0");
    expect(screen.getByRole("heading")).toHaveClass("truncate");
    expect(screen.getByText("12 issues")).toHaveClass("shrink-0");
  });

  it("wraps trailing controls instead of overflowing a narrowed column", () => {
    render(
      <IssuesHeader
        title="Cycle"
        totalCount={12}
        offered={LAYOUT_ORDER}
        active="list"
        onSwitch={() => {}}
        actions={<button type="button">Display</button>}
      />
    );
    expect(screen.getByTestId("issues-header")).toHaveClass("flex-wrap");
    const actions = screen.getByTestId("issues-header-actions");
    expect(actions).toHaveClass("flex-wrap");
    expect(actions).toHaveClass("justify-end");
    // shrink-0 would pin the row at its min-content and defeat the wrap.
    expect(actions).toHaveClass("min-w-0");
    expect(actions).not.toHaveClass("shrink-0");
  });

  it("hides the switcher while keeping the title, count and actions", () => {
    render(
      <IssuesHeader
        title="Website"
        totalCount={3}
        offered={LAYOUT_ORDER}
        active="kanban"
        onSwitch={() => {}}
        actions={<button type="button">Display</button>}
        hideSwitcher
      />
    );
    expect(screen.getByRole("heading", { name: "Website" })).toBeInTheDocument();
    expect(screen.getByText("3 issues")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Display" })).toBeInTheDocument();
    expect(screen.queryByTestId("layout-switcher-segmented")).not.toBeInTheDocument();
    expect(screen.queryByTestId("layout-switcher-dropdown")).not.toBeInTheDocument();
  });
});
