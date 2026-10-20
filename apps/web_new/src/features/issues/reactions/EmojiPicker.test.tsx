// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Emoji picker (ISS-203/204): open, search, pick and close.
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { EMOJI_PICKER_LABEL, EMOJI_SEARCH_LABEL, EmojiPicker } from "./EmojiPicker.js";
import { REACTION_CHOICES } from "./model.js";

describe("EmojiPicker", () => {
  it("offers every choice behind a dialog trigger", async () => {
    const user = userEvent.setup();
    render(<EmojiPicker onPick={vi.fn()} />);
    const trigger = screen.getByRole("button", { name: EMOJI_PICKER_LABEL });
    expect(trigger).toHaveAttribute("aria-haspopup", "dialog");
    await user.click(trigger);
    const list = await screen.findByRole("listbox", { name: "Emoji" });
    expect(within(list).getAllByRole("option")).toHaveLength(REACTION_CHOICES.length);
  });

  it("hands back the character and closes the panel", async () => {
    const user = userEvent.setup();
    const onPick = vi.fn();
    render(<EmojiPicker onPick={onPick} />);
    await user.click(screen.getByRole("button", { name: EMOJI_PICKER_LABEL }));
    await user.click(await screen.findByRole("option", { name: "👍" }));
    expect(onPick).toHaveBeenCalledWith("👍");
    expect(screen.queryByRole("listbox", { name: "Emoji" })).not.toBeInTheDocument();
  });

  it("narrows the grid by the search text", async () => {
    const user = userEvent.setup();
    render(<EmojiPicker onPick={vi.fn()} />);
    await user.click(screen.getByRole("button", { name: EMOJI_PICKER_LABEL }));
    await user.type(screen.getByRole("searchbox", { name: EMOJI_SEARCH_LABEL }), "❤");
    const list = screen.getByRole("listbox", { name: "Emoji" });
    expect(
      within(list)
        .getAllByRole("option")
        .map((option) => option.textContent)
    ).toEqual(["❤️"]);
    await user.clear(screen.getByRole("searchbox", { name: EMOJI_SEARCH_LABEL }));
    expect(within(screen.getByRole("listbox", { name: "Emoji" })).getAllByRole("option")).toHaveLength(
      REACTION_CHOICES.length
    );
  });

  it("says so when nothing matches, and clears the query on close", async () => {
    const user = userEvent.setup();
    render(<EmojiPicker onPick={vi.fn()} />);
    const trigger = screen.getByRole("button", { name: EMOJI_PICKER_LABEL });
    await user.click(trigger);
    await user.type(screen.getByRole("searchbox", { name: EMOJI_SEARCH_LABEL }), "zzzz");
    expect(screen.getByText("No emoji match that search.")).toBeInTheDocument();
    await user.click(trigger);
    await user.click(trigger);
    expect(screen.getByRole("searchbox", { name: EMOJI_SEARCH_LABEL })).toHaveValue("");
    expect(screen.getByRole("listbox", { name: "Emoji" })).toBeInTheDocument();
  });

  it("cannot be opened while disabled", async () => {
    const user = userEvent.setup();
    render(<EmojiPicker onPick={vi.fn()} disabled />);
    const trigger = screen.getByRole("button", { name: EMOJI_PICKER_LABEL });
    expect(trigger).toBeDisabled();
    await user.click(trigger);
    expect(screen.queryByRole("listbox", { name: "Emoji" })).not.toBeInTheDocument();
  });

  it("takes a custom trigger label", () => {
    render(<EmojiPicker onPick={vi.fn()} label="React with emoji" />);
    expect(screen.getByRole("button", { name: "React with emoji" })).toBeInTheDocument();
  });
});
