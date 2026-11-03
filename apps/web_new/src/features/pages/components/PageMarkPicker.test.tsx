// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Create dialog's mark picker (parity/specs/pages.md §3.6 and §5.2;
// PAGE-008): one emoji or one named symbol with a colour, chosen from the
// settings area's shared catalogue, and removable back to no mark.

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";

import { PageMarkPicker, pageMarkColor, pageMarkTriggerGlyph } from "./PageMarkPicker.js";

function Harness({ initial = {} }: { initial?: Record<string, unknown> }): React.ReactElement {
  const [draft, setDraft] = useState<Record<string, unknown>>(initial);
  return (
    <div>
      <PageMarkPicker draft={draft} onDraftChange={setDraft} />
      <output data-testid="draft">{JSON.stringify(draft)}</output>
    </div>
  );
}

function draftOf(): Record<string, unknown> {
  return JSON.parse(screen.getByTestId("draft").textContent ?? "{}") as Record<string, unknown>;
}

describe("pageMarkTriggerGlyph", () => {
  it("reads the emoji, icon and empty faces of a draft", () => {
    expect(pageMarkTriggerGlyph({ in_use: "emoji", emoji: { value: "1f600" } })).toEqual({
      kind: "emoji",
      value: "😀",
    });
    expect(pageMarkTriggerGlyph({ in_use: "icon", icon: { name: "rocket_launch" } })).toEqual({
      kind: "icon",
      value: "rocket_launch",
    });
    expect(pageMarkTriggerGlyph({})).toEqual({ kind: "none", value: "" });
  });

  it("starts a colour pick from the draft's own icon colour, else the default", () => {
    expect(pageMarkColor({ in_use: "icon", icon: { name: "rocket_launch", color: "#0ea5e9" } })).toBe("#0ea5e9");
    expect(pageMarkColor({ in_use: "icon", icon: { name: "rocket_launch" } })).toBe("#6366f1");
    expect(pageMarkColor({})).toBe("#6366f1");
    expect(pageMarkColor({ in_use: "emoji", emoji: { value: "128293" } })).toBe("#6366f1");
  });
});

describe("PageMarkPicker", () => {
  it("offers an empty page sheet until a mark is chosen", () => {
    render(<Harness />);
    expect(screen.getByRole("button", { name: "Choose a page mark" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Remove mark" })).not.toBeInTheDocument();
  });

  it("picks an icon in the default colour", () => {
    render(<Harness />);
    fireEvent.click(screen.getByRole("button", { name: "Choose a page mark" }));
    expect(screen.getByRole("tab", { name: "Icon" })).toHaveAttribute("aria-selected", "true");

    fireEvent.change(screen.getByPlaceholderText("Search icons"), { target: { value: "rocket" } });
    fireEvent.click(screen.getByRole("button", { name: "Rocket launch" }));
    expect(draftOf()).toEqual({ in_use: "icon", icon: { name: "rocket_launch", color: "#6366f1" } });
    expect(screen.getByRole("button", { name: "Change the page mark" })).toBeInTheDocument();
  });

  it("carries a freshly chosen colour onto the icon", () => {
    render(<Harness />);
    fireEvent.click(screen.getByRole("button", { name: "Choose a page mark" }));
    fireEvent.click(screen.getByRole("button", { name: "Pink" }));
    expect(screen.getByRole("button", { name: "Pink" })).toHaveAttribute("aria-pressed", "true");
    fireEvent.change(screen.getByPlaceholderText("Search icons"), { target: { value: "rocket" } });
    fireEvent.click(screen.getByRole("button", { name: "Rocket launch" }));
    expect(draftOf()).toEqual({ in_use: "icon", icon: { name: "rocket_launch", color: "#ec4899" } });
  });

  it("says when no icon matches and clears the search on reopening", () => {
    render(<Harness />);
    fireEvent.click(screen.getByRole("button", { name: "Choose a page mark" }));
    fireEvent.change(screen.getByPlaceholderText("Search icons"), { target: { value: "zzzz" } });
    expect(screen.getByText("No icon matches that search.")).toBeInTheDocument();

    fireEvent.click(screen.getByTestId("page-create-mark"));
    fireEvent.click(screen.getByTestId("page-create-mark"));
    expect(screen.getByPlaceholderText("Search icons")).toHaveValue("");
  });

  it("picks a catalogue emoji", () => {
    render(<Harness />);
    fireEvent.click(screen.getByRole("button", { name: "Choose a page mark" }));
    fireEvent.click(screen.getByRole("tab", { name: "Emoji" }));
    expect(screen.getByPlaceholderText("Search or type an emoji")).toBeInTheDocument();
    fireEvent.change(screen.getByPlaceholderText("Search or type an emoji"), { target: { value: "fire" } });
    fireEvent.click(screen.getByRole("button", { name: "fire" }));
    expect(draftOf()).toEqual({ in_use: "emoji", emoji: { value: "128293" } });
  });

  it("offers an emoji typed outside the catalogue and says when nothing matches", () => {
    render(<Harness />);
    fireEvent.click(screen.getByRole("button", { name: "Choose a page mark" }));
    fireEvent.click(screen.getByRole("tab", { name: "Emoji" }));

    fireEvent.change(screen.getByPlaceholderText("Search or type an emoji"), { target: { value: "🦄x" } });
    expect(screen.getByText("No emoji matches that search.")).toBeInTheDocument();

    fireEvent.change(screen.getByPlaceholderText("Search or type an emoji"), { target: { value: "🧁" } });
    fireEvent.click(screen.getByRole("button", { name: "Use 🧁" }));
    expect(draftOf()).toEqual({ in_use: "emoji", emoji: { value: "129473" } });
  });

  it("marks the stored emoji and icon as the selected tiles", () => {
    const { unmount } = render(<Harness initial={{ in_use: "emoji", emoji: { value: "128293" } }} />);
    fireEvent.click(screen.getByRole("button", { name: "Change the page mark" }));
    fireEvent.click(screen.getByRole("tab", { name: "Emoji" }));
    expect(screen.getByRole("button", { name: "fire" })).toHaveAttribute("aria-pressed", "true");
    unmount();

    render(<Harness initial={{ in_use: "icon", icon: { name: "rocket_launch", color: "#0ea5e9" } }} />);
    fireEvent.click(screen.getByRole("button", { name: "Change the page mark" }));
    fireEvent.change(screen.getByPlaceholderText("Search icons"), { target: { value: "rocket" } });
    expect(screen.getByRole("button", { name: "Rocket launch" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Sky" })).toHaveAttribute("aria-pressed", "true");
  });

  it("draws nothing for a stored icon the glyph map does not know", () => {
    render(<Harness initial={{ in_use: "icon", icon: { name: "no_such_symbol", color: "#0ea5e9" } }} />);
    const trigger = screen.getByTestId("page-create-mark");
    expect(trigger.querySelector("svg")).toBeNull();
    fireEvent.click(trigger);
    expect(screen.getByRole("button", { name: "Sky" })).toHaveAttribute("aria-pressed", "true");
  });

  it("removes a chosen mark back to none", () => {
    const onDraftChange = vi.fn();
    render(
      <PageMarkPicker
        draft={{ in_use: "icon", icon: { name: "rocket_launch", color: "#6366f1" } }}
        onDraftChange={onDraftChange}
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "Change the page mark" }));
    fireEvent.click(screen.getByRole("button", { name: "Remove mark" }));
    expect(onDraftChange).toHaveBeenCalledWith({});
  });
});

describe("PageMarkPicker emoji typed outside the catalogue", () => {
  it("offers the typed glyph once and does not offer it twice", () => {
    render(<Harness />);
    fireEvent.click(screen.getByRole("button", { name: "Choose a page mark" }));
    fireEvent.click(screen.getByRole("tab", { name: "Emoji" }));
    // A catalogue hit: the tile is enough and no extra entry is offered.
    fireEvent.change(screen.getByPlaceholderText("Search or type an emoji"), { target: { value: "🚀" } });
    expect(screen.queryByRole("button", { name: "Use 🚀" })).not.toBeInTheDocument();
    // A glyph outside the catalogue is offered as its own entry.
    fireEvent.change(screen.getByPlaceholderText("Search or type an emoji"), { target: { value: "🫠" } });
    fireEvent.click(screen.getByRole("button", { name: "Use 🫠" }));
    expect(draftOf()).toEqual({ in_use: "emoji", emoji: { value: "129760" } });
  });
});

describe("PageMarkPicker trigger overrides", () => {
  it("carries a caller's trigger test id, class and name", () => {
    render(
      <PageMarkPicker
        draft={{}}
        onDraftChange={vi.fn()}
        triggerTestId="page-mark-trigger"
        triggerClassName="size-14"
        triggerLabel="Add a page mark"
      />
    );
    const trigger = screen.getByTestId("page-mark-trigger");
    expect(trigger).toHaveAccessibleName("Add a page mark");
    expect(trigger).toHaveClass("size-14");
  });
});
