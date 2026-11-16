// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Visibility rail (parity/specs/agents.md §5, AGT-016): one checkbox row
// per installed definition with its color mark, show-all/hide-all, the
// empty state, and the catalog ordering used by the rows.
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { VisibilityRail, railCalendars } from "./VisibilityRail.js";

const CALENDARS = [
  { id: "s1", name: "Alpha", color: "#111111" },
  { id: "s2", name: "Beta", color: "#222222" },
];

function setup(overrides: Partial<React.ComponentProps<typeof VisibilityRail>> = {}) {
  const props = {
    calendars: CALENDARS,
    isVisible: () => true,
    onToggle: vi.fn(),
    onShowAll: vi.fn(),
    onHideAll: vi.fn(),
    ...overrides,
  };
  render(<VisibilityRail {...props} />);
  return props;
}

describe("VisibilityRail", () => {
  it("lists a checkbox row per calendar with its color mark", () => {
    setup({ isVisible: (id) => id !== "s2" });
    const rail = screen.getByRole("complementary", { name: "Calendars" });
    const rows = within(rail).getAllByRole("checkbox");
    expect(rows).toHaveLength(2);
    expect(rows[0]).toBeChecked();
    expect(rows[1]).not.toBeChecked();
    const label = screen.getByText("Beta").closest("label");
    expect(label).not.toBeNull();
    const mark = (label as HTMLElement).querySelector('span[aria-hidden="true"]') as HTMLElement;
    expect(mark.style.backgroundColor).toBe("rgb(34, 34, 34)");
  });

  it("hides itself below the desktop breakpoint", () => {
    setup();
    expect(screen.getByRole("complementary", { name: "Calendars" })).toHaveClass("hidden", "md:flex");
  });

  it("toggles the row it is clicked on", async () => {
    const props = setup();
    await userEvent.click(screen.getByRole("checkbox", { name: "Alpha" }));
    expect(props.onToggle).toHaveBeenCalledWith("s1");
  });

  it("shows all and hides all", async () => {
    const props = setup();
    await userEvent.click(screen.getByRole("button", { name: "Show all" }));
    await userEvent.click(screen.getByRole("button", { name: "Hide all" }));
    expect(props.onShowAll).toHaveBeenCalledTimes(1);
    expect(props.onHideAll).toHaveBeenCalledTimes(1);
  });

  it("shows an empty state and disables the bulk controls without installs", () => {
    setup({ calendars: [] });
    expect(screen.getByText("No schedulers are installed on this project.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Show all" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Hide all" })).toBeDisabled();
  });
});

describe("railCalendars", () => {
  it("orders rows by the catalog, keeping unknown installs last in server order", () => {
    const rows = railCalendars(
      [
        { scheduler: "s3", scheduler_name: "Gamma", scheduler_color: "#3" },
        { scheduler: "s2", scheduler_name: "Beta", scheduler_color: "#2" },
        { scheduler: "s1", scheduler_name: "Alpha", scheduler_color: "#1" },
      ],
      ["s1", "s2"]
    );
    expect(rows.map((row) => row.name)).toEqual(["Alpha", "Beta", "Gamma"]);
    expect(rows[0]).toEqual({ id: "s1", name: "Alpha", color: "#1" });
  });

  it("keeps two installs missing from the catalog in their server order", () => {
    const rows = railCalendars(
      [
        { scheduler: "x1", scheduler_name: "First", scheduler_color: "#1" },
        { scheduler: "x2", scheduler_name: "Second", scheduler_color: "#2" },
      ],
      []
    );
    expect(rows.map((row) => row.name)).toEqual(["First", "Second"]);
  });
});
