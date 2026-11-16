// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Emblem picker (PSET-007): a searchable emoji alphabet wide enough to
// reach any common character (plus a typed character outside it), and the
// Material symbol surface with a colour swatch. A pick hands the draft
// mark back and closes; a dismissed dialog clears its query. No gallery or
// provider search exists here.
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { EmblemPicker, IconTile, initialEmblemMode } from "./EmblemPicker.js";
import {
  EMBLEM_ICON_COLORS,
  emojiFromQuery,
  iconNameLabel,
  searchEmblemEmoji,
  searchEmblemIcons,
  selectedEmblemIcon,
} from "../emblemCatalog.js";

describe("searchEmblemEmoji", () => {
  it("offers the whole alphabet for an empty or blank query", () => {
    const all = searchEmblemEmoji("");
    expect(all.length).toBeGreaterThan(100);
    expect(searchEmblemEmoji("  ")).toEqual(all);
    expect(all.map((entry) => entry.glyph)).toContain("🚀");
  });

  it("matches a glyph exactly and a name by fragment", () => {
    expect(searchEmblemEmoji(" 🚀 ").map((entry) => entry.glyph)).toEqual(["🚀"]);
    expect(searchEmblemEmoji("rocket").map((entry) => entry.glyph)).toEqual(["🚀"]);
    expect(searchEmblemEmoji("zzz")).toEqual([]);
  });
});

describe("emojiFromQuery", () => {
  it("accepts a character outside the alphabet", () => {
    expect(emojiFromQuery("🦖")).toBe("🦖");
    expect(emojiFromQuery(" 🛰️ ")).toBe("🛰️");
  });

  it("refuses words, digits and long sequences", () => {
    expect(emojiFromQuery("")).toBeNull();
    expect(emojiFromQuery("rocket")).toBeNull();
    expect(emojiFromQuery("123")).toBeNull();
    expect(emojiFromQuery("🔥🔥🔥🔥🔥")).toBeNull();
    expect(emojiFromQuery("🔥x")).toBeNull();
    expect(emojiFromQuery("👨‍👩‍👧")).toBe("👨‍👩‍👧");
    expect(emojiFromQuery("👍🏽")).toBe("👍🏽");
    expect(emojiFromQuery("🏴󠁧󠁢󠁳󠁣󠁴󠁿")).toBe("🏴󠁧󠁢󠁳󠁣󠁴󠁿");
  });
});

describe("searchEmblemIcons", () => {
  it("lists every stored symbol and narrows by name", () => {
    expect(searchEmblemIcons("").length).toBeGreaterThan(50);
    expect(searchEmblemIcons("rocket").map((icon) => icon.name)).toEqual(["rocket_launch"]);
    expect(searchEmblemIcons("Rocket").map((icon) => icon.name)).toEqual(["rocket_launch"]);
    expect(searchEmblemIcons("nope")).toEqual([]);
  });
});

describe("iconNameLabel", () => {
  it("title-cases the first word and keeps the rest", () => {
    expect(iconNameLabel("rocket_launch")).toBe("Rocket launch");
    expect(iconNameLabel("")).toBe("");
  });
});

describe("selectedEmblemIcon", () => {
  it("names the draft's symbol and its colour", () => {
    expect(selectedEmblemIcon({ in_use: "icon", icon: { name: "anchor", color: "#8b5cf6" } })).toEqual({
      name: "anchor",
      color: "#8b5cf6",
    });
    expect(selectedEmblemIcon({ in_use: "icon", icon: { name: "anchor" } })).toEqual({
      name: "anchor",
      color: null,
    });
    expect(selectedEmblemIcon({ in_use: "emoji", emoji: { value: "128640" } })).toBeNull();
  });
});

describe("IconTile", () => {
  it("draws a known symbol and leaves an unknown one blank", () => {
    const { container } = render(
      <>
        <IconTile icon={{ name: "anchor", label: "Anchor" }} color="#6366f1" />
        <IconTile icon={{ name: "not_a_symbol", label: "Nope" }} color={null} />
      </>
    );
    expect(container.querySelectorAll("svg")).toHaveLength(1);
    expect(container.querySelectorAll("span")[1]).toHaveAttribute("style", expect.stringContaining("inherit"));
  });
});

describe("initialEmblemMode", () => {
  it("opens on the draft's kind", () => {
    expect(initialEmblemMode({ in_use: "icon", icon: { name: "star" } })).toBe("icon");
    expect(initialEmblemMode({ in_use: "emoji", emoji: { value: "128293" } })).toBe("emoji");
    expect(initialEmblemMode({})).toBe("emoji");
  });
});

describe("EmblemPicker", () => {
  function renderPicker(logoProps: Record<string, unknown> = {}) {
    const onPick = vi.fn();
    const onOpenChange = vi.fn();
    const view = render(<EmblemPicker open onOpenChange={onOpenChange} logoProps={logoProps} onPick={onPick} />);
    return { onPick, onOpenChange, view };
  }

  it("hands a picked emoji back as the stored mark and closes", async () => {
    const user = userEvent.setup();
    const { onPick, onOpenChange } = renderPicker();
    await user.click(screen.getByRole("button", { name: "🌿" }));
    expect(onPick).toHaveBeenCalledWith({ in_use: "emoji", emoji: { value: "127807" } });
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("reaches a rocket, which the create dialog's alphabet does not offer", async () => {
    const user = userEvent.setup();
    const { onPick } = renderPicker();
    await user.click(screen.getByRole("button", { name: "🚀" }));
    expect(onPick).toHaveBeenCalledWith({ in_use: "emoji", emoji: { value: "128640" } });
  });

  it("finds a rocket by name and offers a typed character outside the alphabet", async () => {
    const user = userEvent.setup();
    const { onPick } = renderPicker();
    const search = screen.getByRole("textbox", { name: "Search" });
    await user.type(search, "rocket");
    expect(screen.getAllByTestId("general-emblem-choice")).toHaveLength(1);
    expect(screen.getByRole("button", { name: "🚀" })).toBeInTheDocument();
    await user.clear(search);
    await user.type(search, "🦖");
    expect(screen.getByText("Use this emoji")).toBeInTheDocument();
    await user.click(screen.getByTestId("general-emblem-typed"));
    expect(onPick).toHaveBeenCalledWith({ in_use: "emoji", emoji: { value: "129430" } });
  });

  it("reports no emoji match for a word", async () => {
    const user = userEvent.setup();
    renderPicker();
    await user.type(screen.getByRole("textbox", { name: "Search" }), "zzz");
    expect(screen.getByText("No emoji match that search.")).toBeInTheDocument();
  });

  it("marks the stored decimal value as the chosen tile", () => {
    renderPicker({ in_use: "emoji", emoji: { value: "128293" } });
    expect(screen.getByRole("button", { name: "🔥" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "🌿" })).toHaveAttribute("aria-pressed", "false");
  });

  it("does not treat an upload mark as a chosen emoji", () => {
    renderPicker({ in_use: "upload", url: "/api/assets/v2/static/emblem/" });
    expect(screen.getByRole("button", { name: "🌿" })).toHaveAttribute("aria-pressed", "false");
  });

  it("picks an icon with the chosen colour", async () => {
    const user = userEvent.setup();
    const { onPick } = renderPicker();
    await user.click(screen.getByRole("tab", { name: "Icon" }));
    await user.click(screen.getByRole("button", { name: "Amber" }));
    await user.type(screen.getByRole("textbox", { name: "Search" }), "rocket");
    const icons = screen.getAllByTestId("general-emblem-icon");
    expect(icons).toHaveLength(1);
    await user.click(icons[0]!);
    expect(onPick).toHaveBeenCalledWith({
      in_use: "icon",
      icon: { name: "rocket_launch", color: EMBLEM_ICON_COLORS[3]?.value },
    });
  });

  it("opens on the icon surface for an icon mark and marks its tile", () => {
    renderPicker({ in_use: "icon", icon: { name: "anchor", color: "#8b5cf6" } });
    expect(screen.getByRole("tab", { name: "Icon" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("button", { name: "Anchor" })).toHaveAttribute("aria-pressed", "true");
  });

  it("reports no icon match", async () => {
    const user = userEvent.setup();
    renderPicker();
    await user.click(screen.getByRole("tab", { name: "Icon" }));
    await user.type(screen.getByRole("textbox", { name: "Search" }), "zzz");
    expect(screen.getByText("No icon matches that search.")).toBeInTheDocument();
  });

  it("closes from its own cancel control", async () => {
    const user = userEvent.setup();
    const { onOpenChange } = renderPicker();
    await user.click(
      within(screen.getByRole("dialog", { name: "Change emblem" })).getByRole("button", { name: "Cancel" })
    );
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("clears its query when reopened", async () => {
    const user = userEvent.setup();
    const onPick = vi.fn();
    const onOpenChange = vi.fn();
    const { rerender } = render(<EmblemPicker open onOpenChange={onOpenChange} logoProps={{}} onPick={onPick} />);
    await user.type(screen.getByRole("textbox", { name: "Search" }), "rocket");
    expect(screen.getAllByTestId("general-emblem-choice")).toHaveLength(1);
    rerender(<EmblemPicker open={false} onOpenChange={onOpenChange} logoProps={{}} onPick={onPick} />);
    rerender(<EmblemPicker open onOpenChange={onOpenChange} logoProps={{}} onPick={onPick} />);
    expect(screen.getByRole("textbox", { name: "Search" })).toHaveValue("");
  });
});
