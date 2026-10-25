// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { peekModeLabel, PeekModeToggle } from "./PeekModeToggle.js";

describe("peekModeLabel", () => {
  it("names every mode", () => {
    expect(peekModeLabel("side")).toBe("Side peek");
    expect(peekModeLabel("modal")).toBe("Modal");
    expect(peekModeLabel("full")).toBe("Full screen");
  });
});

describe("PeekModeToggle", () => {
  it("switches modes through the menu", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    const view = render(<PeekModeToggle value="side" onChange={onChange} />);
    await user.click(screen.getByRole("button", { name: "Display mode" }));
    await user.click(await screen.findByRole("menuitem", { name: "Full screen" }));
    expect(onChange).toHaveBeenCalledWith("full");
    view.rerender(<PeekModeToggle value="full" onChange={onChange} />);
    await user.click(screen.getByRole("button", { name: "Display mode" }));
    await user.click(await screen.findByRole("menuitem", { name: "Modal" }));
    expect(onChange).toHaveBeenCalledWith("modal");
    fireEvent.keyDown(document, { key: "Escape" });
  });
});
