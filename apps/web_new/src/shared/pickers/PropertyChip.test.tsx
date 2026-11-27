// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { describeChip, PropertyChip } from "./PropertyChip.js";

describe("describeChip", () => {
  it("names the field and value before the sentence", () => {
    expect(describeChip("Runs on", "Codex engine", "The agent/runner that will execute this work item.")).toBe(
      "Runs on: Codex engine — The agent/runner that will execute this work item."
    );
  });

  it("reads None when nothing is set", () => {
    expect(describeChip("State", null, "The workflow stage this work item is in.")).toBe(
      "State: None — The workflow stage this work item is in."
    );
  });
});

describe("PropertyChip", () => {
  it("names itself from the label, not the visible text", () => {
    render(<PropertyChip label="State: Backlog" text="Backlog" title="Backlog" />);
    const chip = screen.getByRole("button", { name: "State: Backlog" });
    expect(chip).toHaveTextContent("Backlog");
    expect(chip).toHaveAttribute("title", "Backlog");
  });

  it("keeps the small control height and truncates long values", () => {
    const { container } = render(<PropertyChip label="Cycle: Sprint" text="A very long cycle name" />);
    const chip = screen.getByRole("button", { name: "Cycle: Sprint" });
    expect(chip.className).toContain("max-w-48");
    const text = container.querySelector("span.truncate");
    expect(text?.textContent).toBe("A very long cycle name");
  });

  it("mutes the empty placeholder", () => {
    render(<PropertyChip label="Assignees" text="Assignees" placeholder />);
    const chip = screen.getByRole("button", { name: "Assignees" });
    expect(chip.querySelector("span.truncate")?.className).toContain("text-(--text-muted)");
  });

  it("renders borderless without text for the picked-assignees case", () => {
    render(
      <PropertyChip label="Assignees: Pilot User" borderless icon={<span data-testid="avatars" />} title="Pilot User" />
    );
    const chip = screen.getByRole("button", { name: "Assignees: Pilot User" });
    expect(chip.querySelector("span.truncate")).toBeNull();
    expect(screen.getByTestId("avatars")).toBeInTheDocument();
    expect(chip).toHaveAttribute("title", "Pilot User");
  });

  it("renders no text node for an empty string either", () => {
    render(<PropertyChip label="Labels" text="" />);
    expect(screen.getByRole("button", { name: "Labels" }).querySelector("span.truncate")).toBeNull();
  });

  it("shows the descriptive tooltip on hover instead of a native title", async () => {
    render(
      <PropertyChip
        label="Runs on: Codex engine"
        text="Codex engine"
        tooltip="Runs on: Codex engine — The agent/runner that will execute this work item."
      />
    );
    const chip = screen.getByRole("button", { name: "Runs on: Codex engine" });
    expect(chip).not.toHaveAttribute("title");
    fireEvent.focus(chip);
    fireEvent.mouseEnter(chip);
    expect(await screen.findByRole("tooltip")).toHaveTextContent(
      "Runs on: Codex engine — The agent/runner that will execute this work item."
    );
  });

  it("passes interaction props through to the button", () => {
    const onClick = vi.fn();
    render(<PropertyChip label="Labels" text="Labels" tabIndex={7} disabled onClick={onClick} data-testid="chip" />);
    const chip = screen.getByRole("button", { name: "Labels" });
    expect(chip).toHaveAttribute("tabindex", "7");
    expect(chip).toBeDisabled();
    fireEvent.click(chip);
    expect(onClick).not.toHaveBeenCalled();
  });
});
