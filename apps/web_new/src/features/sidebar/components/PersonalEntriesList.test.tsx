// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { PersonalEntriesList } from "./PersonalEntriesList.js";

const ENTRIES = [
  { key: "a", label: "Alpha", enabled: true },
  { key: "b", label: "Beta", enabled: false },
  { key: "c", label: "Gamma", enabled: true },
];

describe("PersonalEntriesList", () => {
  it("renders rows with their visibility checkboxes", () => {
    render(<PersonalEntriesList entries={ENTRIES} onToggle={() => undefined} onMove={() => undefined} />);
    expect(screen.getAllByTestId("personal-row")).toHaveLength(3);
    expect(screen.getByRole("checkbox", { name: "Alpha" })).toBeChecked();
    expect(screen.getByRole("checkbox", { name: "Beta" })).not.toBeChecked();
  });

  it("reports toggles with the next state", async () => {
    const user = userEvent.setup();
    const onToggle = vi.fn();
    render(<PersonalEntriesList entries={ENTRIES} onToggle={onToggle} onMove={() => undefined} />);
    await user.click(screen.getByRole("checkbox", { name: "Beta" }));
    expect(onToggle).toHaveBeenCalledWith("b", true);
  });

  it("moves a dragged row onto its drop target", () => {
    const onMove = vi.fn();
    render(<PersonalEntriesList entries={ENTRIES} onToggle={() => undefined} onMove={onMove} />);
    const rows = screen.getAllByTestId("personal-row");
    fireEvent.dragStart(rows[0]!);
    fireEvent.dragOver(rows[2]!);
    fireEvent.drop(rows[2]!);
    expect(onMove).toHaveBeenCalledWith("a", "c");
    expect(onMove).toHaveBeenCalledTimes(1);
  });

  it("ignores drops with no drag source", () => {
    const onMove = vi.fn();
    render(<PersonalEntriesList entries={ENTRIES} onToggle={() => undefined} onMove={onMove} />);
    fireEvent.drop(screen.getAllByTestId("personal-row")[1]!);
    expect(onMove).not.toHaveBeenCalled();
  });

  it("locks rows while disabled", () => {
    render(<PersonalEntriesList entries={ENTRIES} disabled onToggle={() => undefined} onMove={() => undefined} />);
    for (const row of screen.getAllByTestId("personal-row")) {
      expect(row).toHaveAttribute("draggable", "false");
    }
    expect(screen.getByRole("checkbox", { name: "Alpha" })).toBeDisabled();
  });
});
