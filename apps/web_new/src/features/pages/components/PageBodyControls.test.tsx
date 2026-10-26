// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page interaction controls (parity/specs/pages.md §6.1–§6.4,
// PAGE-037–044): the palette, block menu, emoji picker, mention rows and
// selection bubble only expose the contract the interaction layer drives.

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { PAGE_SLASH_ENTRIES } from "../lib/pageBlocks.js";
import {
  PAGE_BUBBLE_COMMANDS,
  PageBlockHandle,
  PageEmojiPicker,
  PageEntityPicker,
  PageMentionSuggestions,
  PageSelectionBubble,
  PageSlashPalette,
  PAGE_MENTION_USERS,
} from "./PageBodyControls.js";

describe("PageSlashPalette", () => {
  it("lists the full catalogue with each label inside its button paragraph", () => {
    render(<PageSlashPalette query="" onPick={vi.fn()} onDismiss={vi.fn()} />);
    const buttons = screen.getByTestId("page-slash-palette").querySelectorAll("button p");
    expect(Array.from(buttons).map((node) => node.textContent)).toEqual(PAGE_SLASH_ENTRIES.map((entry) => entry.label));
  });

  it("filters and reports the picked entry", () => {
    const onPick = vi.fn();
    render(<PageSlashPalette query="head" onPick={onPick} onDismiss={vi.fn()} />);
    expect(screen.getAllByRole("button", { name: /Heading/ })).toHaveLength(6);
    const heading = screen.getByRole("button", { name: "Heading 2" });
    fireEvent.mouseDown(heading);
    expect(onPick).toHaveBeenCalledWith(expect.objectContaining({ label: "Heading 2" }));
    fireEvent.click(screen.getByRole("button", { name: "Close block menu" }));
  });

  it("shows a no-match message and a dismiss control", () => {
    const onDismiss = vi.fn();
    render(<PageSlashPalette query="zzz" onPick={vi.fn()} onDismiss={onDismiss} />);
    expect(screen.getByText("No blocks match.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Close block menu" }));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });
});

describe("PageBlockHandle", () => {
  it("opens the pinned block menu and reports Delete and Duplicate", () => {
    const onSelect = vi.fn();
    render(<PageBlockHandle onSelect={onSelect} />);
    fireEvent.mouseDown(screen.getByTestId("page-block-handle"));
    fireEvent.click(screen.getByTestId("page-block-handle"));
    expect(screen.getAllByRole("menuitem").map((node) => node.textContent)).toEqual([
      "Delete",
      "Duplicate",
      "Move up",
      "Move down",
    ]);
    fireEvent.mouseDown(screen.getByRole("menuitem", { name: "Delete" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Duplicate" }));
    expect(onSelect).toHaveBeenCalledWith("Duplicate");
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });
});

describe("PageEmojiPicker", () => {
  it("reports the glyph of the chosen picker item", () => {
    const onPick = vi.fn();
    const onDismiss = vi.fn();
    render(<PageEmojiPicker onPick={onPick} onDismiss={onDismiss} />);
    const first = screen.getByTestId("page-emoji-picker").querySelector("button[type=button][id^=emoji-item-]");
    if (first === null) throw new Error("the picker has no entries");
    fireEvent.mouseDown(first);
    expect(onPick).toHaveBeenCalledWith(first.textContent);
    fireEvent.click(screen.getByRole("button", { name: "Dismiss emoji picker" }));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });
});

describe("PageEntityPicker", () => {
  it("renders the project key and title for each row and reports the pick", () => {
    const onPick = vi.fn();
    const onQueryChange = vi.fn();
    render(
      <PageEntityPicker
        state={{
          query: "fix",
          choices: [
            {
              id: "iss-1",
              name: "Fix login",
              sequenceId: 42,
              projectId: "p-1",
              projectIdentifier: "WEB",
              kind: "issue",
              workspaceSlug: "acme",
            },
          ],
          onQueryChange,
          onPick,
          onDismiss: vi.fn(),
        }}
      />
    );
    expect(screen.getByTestId("page-entity-picker")).toBeInTheDocument();
    const row = screen.getByRole("button", { name: /WEB-42 Fix login/ });
    fireEvent.mouseDown(row);
    expect(onPick).toHaveBeenCalledWith(expect.objectContaining({ id: "iss-1" }));
    fireEvent.change(screen.getByRole("searchbox", { name: "Search work items" }), { target: { value: "ship" } });
    expect(onQueryChange).toHaveBeenCalledWith("ship");
  });

  it("shows an empty state and a dismiss control", () => {
    const onDismiss = vi.fn();
    render(
      <PageEntityPicker state={{ query: "zzz", choices: [], onQueryChange: vi.fn(), onPick: vi.fn(), onDismiss }} />
    );
    expect(screen.getByText("No work items match.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Dismiss work-item picker" }));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });
});

describe("PageMentionSuggestions", () => {
  it("renders custom avatar-and-name rows under the Users heading", () => {
    const onPick = vi.fn();
    render(
      <PageMentionSuggestions
        state={{
          query: "ad",
          open: true,
          people: [{ id: "u-1", name: "Ada", avatarUrl: null }],
          onPick,
          onDismiss: vi.fn(),
        }}
      />
    );
    expect(screen.getByText(PAGE_MENTION_USERS)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Ada/ })).toHaveAttribute("id", "mention-item-0");
    fireEvent.mouseDown(screen.getByRole("button", { name: /Ada/ }));
    expect(onPick).toHaveBeenCalledWith(expect.objectContaining({ id: "u-1", name: "Ada" }));
    fireEvent.click(screen.getByRole("button", { name: "Dismiss mentions" }));
  });

  it("renders nothing when closed or empty", () => {
    const { rerender } = render(
      <PageMentionSuggestions
        state={{
          query: "",
          open: false,
          people: [{ id: "u-1", name: "Ada", avatarUrl: null }],
          onPick: vi.fn(),
          onDismiss: vi.fn(),
        }}
      />
    );
    expect(screen.queryByTestId("page-mention-suggestions")).not.toBeInTheDocument();
    rerender(
      <PageMentionSuggestions state={{ query: "", open: true, people: [], onPick: vi.fn(), onDismiss: vi.fn() }} />
    );
    expect(screen.queryByTestId("page-mention-suggestions")).not.toBeInTheDocument();
  });
});

describe("mention rows with avatars", () => {
  it("paints the avatar when the member has a source", () => {
    render(
      <PageMentionSuggestions
        state={{
          query: "ad",
          open: true,
          people: [{ id: "u-1", name: "Ada", avatarUrl: "https://example.com/a.png" }],
          onPick: vi.fn(),
          onDismiss: vi.fn(),
        }}
      />
    );
    expect(screen.getByRole("button", { name: /Ada/ }).querySelector("img")).not.toBeNull();
  });
});

describe("PageSelectionBubble", () => {
  function link(overrides: Partial<React.ComponentProps<typeof PageSelectionBubble>["link"]> = {}) {
    return {
      href: "https://example.com",
      open: false,
      onApply: vi.fn(),
      onRemove: vi.fn(),
      ...overrides,
    };
  }

  it("reports bubble formatting commands in order and opens the link editor", () => {
    const onCommand = vi.fn();
    const onToggleLink = vi.fn();
    render(<PageSelectionBubble link={link()} onCommand={onCommand} onToggleLink={onToggleLink} />);
    const buttons = PAGE_BUBBLE_COMMANDS.map(({ label }) => screen.getByRole("button", { name: label }));
    expect(buttons).toHaveLength(PAGE_BUBBLE_COMMANDS.length);
    fireEvent.mouseDown(buttons[2] as HTMLElement);
    fireEvent.click(buttons[2] as HTMLElement);
    expect(onCommand).toHaveBeenCalledWith("underline");
    fireEvent.mouseDown(screen.getByRole("button", { name: "Link" }));
    fireEvent.click(screen.getByRole("button", { name: "Link" }));
    expect(onToggleLink).toHaveBeenCalledTimes(1);
  });

  it("prefills the link and applies or removes it", () => {
    const onApply = vi.fn();
    const onRemove = vi.fn();
    render(
      <PageSelectionBubble link={link({ open: true, onApply, onRemove })} onCommand={vi.fn()} onToggleLink={vi.fn()} />
    );
    const input = screen.getByRole("textbox", { name: "Paste a link" });
    expect(input).toHaveValue("https://example.com");
    fireEvent.change(input, { target: { value: "https://example.com/edited" } });
    fireEvent.keyDown(input, { key: "Tab" });
    expect(onApply).not.toHaveBeenCalled();
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onApply).toHaveBeenCalledWith("https://example.com/edited");
    fireEvent.click(screen.getByRole("button", { name: "Apply link" }));
    expect(onApply).toHaveBeenCalledTimes(2);
    fireEvent.click(screen.getByRole("button", { name: "Remove link" }));
    expect(onRemove).toHaveBeenCalledTimes(1);
  });
});
