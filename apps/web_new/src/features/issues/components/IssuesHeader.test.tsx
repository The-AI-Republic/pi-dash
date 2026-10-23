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
