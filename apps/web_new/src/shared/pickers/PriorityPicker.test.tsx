// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { PriorityPicker } from "./PriorityPicker.js";

function optionNames(): string[] {
  return within(screen.getByRole("listbox"))
    .getAllByRole("option")
    .map((row) => row.querySelector(":scope > span.truncate")?.textContent ?? "");
}

describe("PriorityPicker", () => {
  it("lists the five levels with a glyph each", () => {
    render(<PriorityPicker value="medium" onChange={() => undefined} />);
    fireEvent.click(screen.getByRole("button", { name: "Medium" }));
    expect(optionNames()).toEqual(["Urgent", "High", "Medium", "Low", "None"]);
    for (const name of ["Urgent", "High", "Medium", "Low", "None"]) {
      expect(screen.getByRole("option", { name }).querySelector("svg")).not.toBeNull();
    }
    expect(screen.getByRole("option", { name: "Medium" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("option", { name: "Urgent" }).querySelector("span[class*='border']")).not.toBeNull();
  });

  it("searches, picks and closes", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<PriorityPicker value="medium" onChange={onChange} />);
    fireEvent.click(screen.getByRole("button", { name: "Medium" }));
    await user.type(screen.getByRole("combobox", { name: "Search priorities" }), "hig");
    expect(optionNames()).toEqual(["High"]);
    fireEvent.click(screen.getByRole("option", { name: "High" }));
    expect(onChange).toHaveBeenCalledWith("high");
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("treats none as a real value", () => {
    const onChange = vi.fn();
    render(<PriorityPicker value="low" onChange={onChange} />);
    fireEvent.click(screen.getByRole("button", { name: "Low" }));
    fireEvent.click(screen.getByRole("option", { name: "None" }));
    expect(onChange).toHaveBeenCalledWith("none");
  });

  it("names a search miss", async () => {
    const user = userEvent.setup();
    render(<PriorityPicker value="medium" onChange={() => undefined} />);
    fireEvent.click(screen.getByRole("button", { name: "Medium" }));
    await user.type(screen.getByRole("combobox", { name: "Search priorities" }), "zzz");
    expect(screen.getByRole("status")).toHaveTextContent("No matching results");
  });

  it("renders icon-only, variant and custom triggers", () => {
    const onChange = vi.fn();
    const { rerender } = render(<PriorityPicker value="urgent" onChange={onChange} iconOnly variant="ghost" />);
    expect(screen.getByRole("button", { name: "Urgent" })).toBeInTheDocument();
    rerender(<PriorityPicker value="urgent" onChange={onChange} trigger={<button type="button">Pick</button>} />);
    fireEvent.click(screen.getByRole("button", { name: "Pick" }));
    expect(screen.getByRole("listbox")).toBeInTheDocument();
  });

  it("disables without reads", () => {
    render(<PriorityPicker value="medium" onChange={() => undefined} disabled />);
    expect(screen.getByRole("button", { name: "Medium" })).toBeDisabled();
  });
});
