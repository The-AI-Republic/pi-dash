// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { LAYOUT_ORDER } from "../layouts/kinds.js";
import { MobileIssuesHeader } from "./MobileIssuesHeader.js";

describe("MobileIssuesHeader", () => {
  it("renders the compact bar under md with the active layout on the trigger", () => {
    render(<MobileIssuesHeader offered={LAYOUT_ORDER} active="kanban" onSwitch={() => {}} />);
    const bar = screen.getByTestId("issues-mobile-header");
    expect(bar).toHaveClass("md:hidden");
    expect(screen.getByTestId("mobile-layout-trigger")).toHaveTextContent("Board");
  });

  it("offers the mobile layout subset in order", () => {
    render(<MobileIssuesHeader offered={LAYOUT_ORDER} active="list" onSwitch={() => {}} />);
    fireEvent.click(screen.getByTestId("mobile-layout-trigger"));
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual(["List", "Board", "Calendar"]);
  });

  it("restricts the menu to the registered mobile layouts", () => {
    render(<MobileIssuesHeader offered={["calendar", "spreadsheet", "list"]} active="list" onSwitch={() => {}} />);
    fireEvent.click(screen.getByTestId("mobile-layout-trigger"));
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual(["List", "Calendar"]);
  });

  it("switches through the menu and no-ops on the active entry", () => {
    const onSwitch = vi.fn();
    render(<MobileIssuesHeader offered={LAYOUT_ORDER} active="list" onSwitch={onSwitch} />);
    fireEvent.click(screen.getByTestId("mobile-layout-trigger"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Calendar" }));
    expect(onSwitch).toHaveBeenCalledTimes(1);
    expect(onSwitch).toHaveBeenCalledWith("calendar");
    fireEvent.click(screen.getByTestId("mobile-layout-trigger"));
    fireEvent.click(screen.getByRole("menuitem", { name: "List" }));
    expect(onSwitch).toHaveBeenCalledTimes(1);
  });

  it("hosts the display and analytics controls their slices provide", () => {
    render(
      <MobileIssuesHeader
        offered={["list"]}
        active="list"
        onSwitch={() => {}}
        displayControl={<button type="button">Display</button>}
        filterControl={<button type="button">Filters</button>}
        analyticsControl={<button type="button">Analytics</button>}
      />
    );
    expect(screen.getByRole("button", { name: "Display" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Filters" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Analytics" })).toBeInTheDocument();
  });

  it("renders no control slots until their slices land", () => {
    render(<MobileIssuesHeader offered={["list"]} active="list" onSwitch={() => {}} />);
    expect(screen.queryByRole("button", { name: "Display" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Filters" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Analytics" })).not.toBeInTheDocument();
  });

  it("renders nothing when the switcher hides and no control is present", () => {
    render(<MobileIssuesHeader offered={LAYOUT_ORDER} active="kanban" onSwitch={() => {}} hideSwitcher />);
    expect(screen.queryByTestId("issues-mobile-header")).not.toBeInTheDocument();
  });

  it("keeps the bar without the menu when a control remains", () => {
    render(
      <MobileIssuesHeader
        offered={LAYOUT_ORDER}
        active="kanban"
        onSwitch={() => {}}
        filterControl={<button type="button">Filters</button>}
        hideSwitcher
      />
    );
    expect(screen.getByTestId("issues-mobile-header")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Filters" })).toBeInTheDocument();
    expect(screen.queryByTestId("mobile-layout-trigger")).not.toBeInTheDocument();
  });

  it("keeps the bar for a lone display control without the menu", () => {
    render(
      <MobileIssuesHeader
        offered={LAYOUT_ORDER}
        active="kanban"
        onSwitch={() => {}}
        displayControl={<button type="button">Display</button>}
        hideSwitcher
      />
    );
    expect(screen.getByTestId("issues-mobile-header")).toBeInTheDocument();
    expect(screen.queryByTestId("mobile-layout-trigger")).not.toBeInTheDocument();
  });

  it("keeps the bar for a lone analytics control without the menu", () => {
    render(
      <MobileIssuesHeader
        offered={LAYOUT_ORDER}
        active="kanban"
        onSwitch={() => {}}
        analyticsControl={<button type="button">Analytics</button>}
        hideSwitcher
      />
    );
    expect(screen.getByTestId("issues-mobile-header")).toBeInTheDocument();
    expect(screen.queryByTestId("mobile-layout-trigger")).not.toBeInTheDocument();
  });
});
