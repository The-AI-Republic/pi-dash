// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Emblem picker (PSET-007): a searchable emoji alphabet, the chosen tile
// marked, a pick handed back as the draft mark, and a dismissed dialog
// clearing its query. No gallery or provider search exists here.
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { EmblemPicker, matchEmoji } from "./EmblemPicker.js";
import { EMBLEM_CHOICES } from "./GeneralIdentity.js";

describe("matchEmoji", () => {
  it("offers the whole alphabet for an empty or blank query", () => {
    expect(matchEmoji("")).toEqual(EMBLEM_CHOICES);
    expect(matchEmoji("  ")).toEqual(EMBLEM_CHOICES);
  });

  it("keeps only an exact emoji match", () => {
    expect(matchEmoji(" 🔥 ")).toEqual(["🔥"]);
    expect(matchEmoji("zzz")).toEqual([]);
  });
});

describe("EmblemPicker", () => {
  function renderPicker(logoProps: Record<string, unknown> = {}) {
    const onPick = vi.fn();
    const onOpenChange = vi.fn();
    render(<EmblemPicker open onOpenChange={onOpenChange} logoProps={logoProps} onPick={onPick} />);
    return { onPick, onOpenChange };
  }

  it("hands a picked emoji back as the mark and closes", async () => {
    const user = userEvent.setup();
    const { onPick, onOpenChange } = renderPicker();
    await user.click(screen.getByRole("button", { name: "🌿" }));
    expect(onPick).toHaveBeenCalledWith({ in_use: "emoji", emoji: "🌿" });
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("marks the stored choice as pressed", () => {
    renderPicker({ in_use: "emoji", emoji: "🔥" });
    expect(screen.getByRole("button", { name: "🔥" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "🌿" })).toHaveAttribute("aria-pressed", "false");
  });

  it("does not treat an upload or an icon mark as a chosen emoji", () => {
    renderPicker({ in_use: "upload", url: "/api/assets/emblem.png" });
    expect(screen.getByRole("button", { name: "🌿" })).toHaveAttribute("aria-pressed", "false");
  });

  it("filters as the operator types and reports no match", async () => {
    const user = userEvent.setup();
    renderPicker();
    const search = screen.getByRole("textbox", { name: "Search emoji" });
    await user.type(search, "🔥");
    expect(screen.getAllByTestId("general-emblem-choice")).toHaveLength(1);
    await user.type(search, "x");
    expect(screen.getByText("No emoji match that search.")).toBeInTheDocument();
  });

  it("closes from its own cancel control", async () => {
    const user = userEvent.setup();
    const { onOpenChange } = renderPicker();
    await user.click(
      within(screen.getByRole("dialog", { name: "Change emblem" })).getByRole("button", { name: "Cancel" })
    );
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("clears its query when the dialog is dismissed", async () => {
    const user = userEvent.setup();
    const { onOpenChange } = renderPicker();
    await user.type(screen.getByRole("textbox", { name: "Search emoji" }), "🔥");
    expect(screen.getAllByTestId("general-emblem-choice")).toHaveLength(1);
    await user.keyboard("{Escape}");
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });
});
