// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { SPACE_REACTION_SHELF, SpaceReactionPicker } from "./SpaceReactionPicker.js";

function renderPicker(onPick: (code: string) => void = vi.fn()) {
  render(<SpaceReactionPicker onPick={onPick} label="Add a reaction" panelLabel="Choose a reaction" testId="add" />);
  return onPick;
}

describe("SpaceReactionPicker", () => {
  it("offers the shelf behind the add trigger", async () => {
    renderPicker();
    const trigger = screen.getByTestId("add");
    expect(trigger).toHaveAccessibleName("Add a reaction");
    expect(trigger).toHaveAttribute("aria-haspopup", "dialog");
    expect(trigger).toHaveAttribute("aria-expanded", "false");
    await userEvent.click(trigger);
    expect(trigger).toHaveAttribute("aria-expanded", "true");
    const panel = screen.getByRole("group", { name: "Choose a reaction" });
    const options = within(panel).getAllByTestId("add-option");
    expect(options.map((option) => option.getAttribute("data-emoji"))).toEqual([...SPACE_REACTION_SHELF]);
  });

  it("hands back the picked emoji's code and closes", async () => {
    const onPick = vi.fn();
    renderPicker(onPick);
    await userEvent.click(screen.getByTestId("add"));
    await userEvent.click(screen.getByRole("button", { name: "🎉" }));
    expect(onPick).toHaveBeenCalledWith("127881");
    expect(screen.queryByRole("group", { name: "Choose a reaction" })).not.toBeInTheDocument();
  });
});
