// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Reaction chips (ISS-203/204): counts, the viewer highlight, the add
// button's visibility and the toggle each chip asks for.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { EMOJI_PICKER_LABEL } from "./EmojiPicker.js";
import { REACTED_CHIP_CLASS, ReactionBar } from "./ReactionBar.js";
import type { ReactionChip } from "./model.js";

function chip(overrides: Partial<ReactionChip> = {}): ReactionChip {
  return { code: "128077", emoji: "👍", count: 2, names: ["Ada L", "Bob B"], reacted: false, ...overrides };
}

describe("ReactionBar", () => {
  it("shows one chip per emoji with its count", () => {
    render(<ReactionBar chips={[chip(), chip({ code: "127881", emoji: "🎉", count: 1 })]} onToggle={vi.fn()} />);
    expect(screen.getByRole("button", { name: "👍 reaction, 2" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "🎉 reaction, 1" })).toBeInTheDocument();
  });

  it("marks the chip the viewer reacted with", () => {
    render(<ReactionBar chips={[chip({ reacted: true })]} onToggle={vi.fn()} />);
    const reacted = screen.getByRole("button", { name: "👍 reaction, 2" });
    expect(reacted).toHaveAttribute("aria-pressed", "true");
    expect(reacted.className.split(" ")).toContain(REACTED_CHIP_CLASS);
  });

  it("leaves a chip the viewer did not react with unmarked", () => {
    render(<ReactionBar chips={[chip()]} onToggle={vi.fn()} />);
    const plain = screen.getByRole("button", { name: "👍 reaction, 2" });
    expect(plain).toHaveAttribute("aria-pressed", "false");
    expect(plain.className).not.toContain(REACTED_CHIP_CLASS);
  });

  it("asks to add when the viewer has not reacted and remove when they have", async () => {
    const user = userEvent.setup();
    const onToggle = vi.fn();
    render(
      <ReactionBar
        chips={[chip(), chip({ code: "127881", emoji: "🎉", count: 1, reacted: true })]}
        onToggle={onToggle}
      />
    );
    await user.click(screen.getByRole("button", { name: "👍 reaction, 2" }));
    await user.click(screen.getByRole("button", { name: "🎉 reaction, 1" }));
    expect(onToggle).toHaveBeenNthCalledWith(1, "128077", "add");
    expect(onToggle).toHaveBeenNthCalledWith(2, "127881", "remove");
  });

  it("keeps the add button beside the chips and hands back a picked emoji", async () => {
    const user = userEvent.setup();
    const onToggle = vi.fn();
    render(<ReactionBar chips={[chip()]} onToggle={onToggle} />);
    await user.click(screen.getByRole("button", { name: EMOJI_PICKER_LABEL }));
    await user.click(await screen.findByRole("option", { name: "👀" }));
    // The picker hands back the character; the wire key is its code points.
    expect(onToggle).toHaveBeenCalledWith("128064", "add");
  });

  it("keeps the chips visible but inert, and drops the add button, when disabled", async () => {
    const user = userEvent.setup();
    const onToggle = vi.fn();
    render(<ReactionBar chips={[chip()]} onToggle={onToggle} disabled />);
    expect(screen.queryByRole("button", { name: EMOJI_PICKER_LABEL })).not.toBeInTheDocument();
    const frozen = screen.getByRole("button", { name: "👍 reaction, 2" });
    expect(frozen).toHaveAttribute("aria-disabled", "true");
    await user.click(frozen);
    expect(onToggle).not.toHaveBeenCalled();
  });

  it("takes a custom label and class for the row", () => {
    const { container } = render(
      <ReactionBar chips={[]} onToggle={vi.fn()} addLabel="React with emoji" className="pl-2" />
    );
    expect(screen.getByRole("button", { name: "React with emoji" })).toBeInTheDocument();
    expect((container.firstElementChild as HTMLElement).className).toContain("pl-2");
  });
});
