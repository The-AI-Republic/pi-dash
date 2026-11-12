// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Event picker (parity/specs/workspace-settings.md §19, WSET-020): mode
// radios, toggles only in individual mode, and the change payloads.
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { EventPicker, EVENT_MODE_LABELS } from "./EventPicker.js";
import { ALL_EVENTS, NO_EVENTS } from "./events.js";

afterEach(() => {
  cleanup();
});

describe("EventPicker", () => {
  it("shows both modes and no toggles in all-events mode", () => {
    render(<EventPicker value={{ mode: "all", flags: ALL_EVENTS }} onChange={() => undefined} />);
    expect(screen.getByRole("radio", { name: EVENT_MODE_LABELS.all })).toBeChecked();
    expect(screen.getByRole("radio", { name: EVENT_MODE_LABELS.individual })).not.toBeChecked();
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
  });

  it("renders the five all-on toggles in individual mode", () => {
    render(<EventPicker value={{ mode: "individual", flags: ALL_EVENTS }} onChange={() => undefined} />);
    const boxes = screen.getAllByRole("checkbox");
    expect(boxes.map((box) => box.getAttribute("id"))).toEqual([
      "project",
      "cycle",
      "issue",
      "module",
      "issue_comment",
    ]);
    expect(boxes.every((box) => (box as HTMLInputElement).checked)).toBe(true);
    expect(screen.getByText("Project creation, updates and removal.")).toBeInTheDocument();
  });

  it("switches modes through the radios", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<EventPicker value={{ mode: "all", flags: ALL_EVENTS }} onChange={onChange} />);
    await user.click(screen.getByRole("radio", { name: EVENT_MODE_LABELS.individual }));
    expect(onChange).toHaveBeenCalledWith({ mode: "individual", flags: ALL_EVENTS });
  });

  it("flips a toggle", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<EventPicker value={{ mode: "individual", flags: ALL_EVENTS }} onChange={onChange} />);
    await user.click(screen.getByRole("checkbox", { name: "Modules" }));
    expect(onChange).toHaveBeenCalledWith({ mode: "individual", flags: { ...ALL_EVENTS, module: false } });
  });

  it("flips a toggle back on", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<EventPicker value={{ mode: "individual", flags: NO_EVENTS }} onChange={onChange} />);
    await user.click(screen.getByRole("checkbox", { name: "Cycles" }));
    expect(onChange).toHaveBeenCalledWith({ mode: "individual", flags: { ...NO_EVENTS, cycle: true } });
  });
});
