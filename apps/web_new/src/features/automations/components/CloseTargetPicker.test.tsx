// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { State } from "@pidash/api-client";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { CloseTargetPicker, STATE_FALLBACK_LABEL } from "./CloseTargetPicker.js";

function state(id: string, name: string): State {
  return {
    id,
    project_id: "223e4567-e89b-12d3-a456-426614174001",
    workspace_id: "323e4567-e89b-12d3-a456-426614174002",
    name,
    color: "#6b7280",
    group: "cancelled",
    default: false,
    description: "",
    sequence: 0,
  };
}

const A = state("123e4567-e89b-12d3-a456-42661417400a", "Dropped");
const B = state("123e4567-e89b-12d3-a456-42661417400b", "Won't do");

describe("CloseTargetPicker", () => {
  it("shows the placeholder, closed, when there are no cancelled states", () => {
    render(<CloseTargetPicker states={[]} selected={null} onSelect={vi.fn()} />);
    const trigger = screen.getByRole("button", { name: STATE_FALLBACK_LABEL });
    expect(trigger).toBeDisabled();
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("stays disabled with a single cancelled state", () => {
    render(<CloseTargetPicker states={[A]} selected={A} onSelect={vi.fn()} />);
    const trigger = screen.getByRole("button", { name: "Dropped" });
    expect(trigger).toBeDisabled();
    expect(trigger.textContent).toBe("Dropped");
  });

  it("lists every cancelled state once a second exists and reports the pick", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    render(<CloseTargetPicker states={[A, B]} selected={A} onSelect={onSelect} />);
    const trigger = screen.getByRole("button", { name: "Dropped" });
    expect(trigger).not.toBeDisabled();
    await user.click(trigger);
    const options = screen.getAllByRole("option");
    expect(options.map((option) => option.textContent)).toEqual(["Dropped", "Won't do"]);
    expect(options[0]).toHaveAttribute("aria-selected", "true");
    await user.click(screen.getByRole("option", { name: "Won't do" }));
    expect(onSelect).toHaveBeenCalledWith(B.id);
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("closes on Escape and an outside click", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    render(<CloseTargetPicker states={[A, B]} selected={A} onSelect={onSelect} />);
    await user.click(screen.getByRole("button", { name: "Dropped" }));
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("listbox")).toBeNull();
    await user.click(screen.getByRole("button", { name: "Dropped" }));
    await user.click(document.body);
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(onSelect).not.toHaveBeenCalled();
  });

  it("toggles closed from the trigger", async () => {
    const user = userEvent.setup();
    render(<CloseTargetPicker states={[A, B]} selected={A} onSelect={vi.fn()} />);
    const trigger = screen.getByRole("button", { name: "Dropped" });
    await user.click(trigger);
    expect(trigger).toHaveAttribute("aria-expanded", "true");
    await user.click(trigger);
    expect(trigger).toHaveAttribute("aria-expanded", "false");
  });
});
