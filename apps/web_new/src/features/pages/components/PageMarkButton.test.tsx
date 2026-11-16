// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail mark control (parity/specs/pages.md §5.2, PAGE-027): the glyph,
// the hover/pinned affordance, and the read-only face that carries no
// picker at all.

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import {
  markDraftOf,
  markTriggerLabel,
  PAGE_MARK_CHANGE_LABEL,
  PAGE_MARK_EMPTY_LABEL,
  PageMarkButton,
  PageMarkGlyph,
} from "./PageMarkButton.js";

const EMOJI = { in_use: "emoji", emoji: { value: "1f680" } };
const ICON = { in_use: "icon", icon: { name: "rocket_launch", color: "#0ea5e9" } };

describe("markDraftOf", () => {
  it("decodes a stored emoji into the picker's emoji draft", () => {
    expect(markDraftOf(EMOJI)).toEqual({ in_use: "emoji", emoji: { value: "🚀" } });
  });

  it("decodes a stored icon with and without a colour", () => {
    expect(markDraftOf(ICON)).toEqual({ in_use: "icon", icon: { name: "rocket_launch", color: "#0ea5e9" } });
    expect(markDraftOf({ in_use: "icon", icon: { name: "star" } })).toEqual({
      in_use: "icon",
      icon: { name: "star" },
    });
  });

  it("reads an uploaded or missing mark as no draft mark", () => {
    expect(markDraftOf({ in_use: "upload", url: "https://cdn.test/mark.png" })).toEqual({});
    expect(markDraftOf({})).toEqual({});
  });
});

describe("markTriggerLabel", () => {
  it("offers to add when there is no mark and change when there is one", () => {
    expect(markTriggerLabel({})).toBe(PAGE_MARK_EMPTY_LABEL);
    expect(markTriggerLabel(EMOJI)).toBe(PAGE_MARK_CHANGE_LABEL);
    expect(markTriggerLabel(ICON)).toBe(PAGE_MARK_CHANGE_LABEL);
  });
});

describe("PageMarkGlyph", () => {
  it("draws an emoji as its glyph", () => {
    render(<PageMarkGlyph logoProps={EMOJI} />);
    expect(screen.getByTestId("page-mark-glyph")).toHaveTextContent("🚀");
  });

  it("draws an uploaded mark as its picture", () => {
    render(<PageMarkGlyph logoProps={{ in_use: "upload", url: "https://cdn.test/mark.png" }} />);
    expect(screen.getByTestId("page-mark-image")).toHaveAttribute("src", "https://cdn.test/mark.png");
  });

  it("draws a symbol once its glyph chunk arrives, and a fallback sheet", async () => {
    render(<PageMarkGlyph logoProps={ICON} />);
    expect(await screen.findByTestId("page-mark-icon", undefined, { timeout: 10_000 })).not.toBeNull();
    render(<PageMarkGlyph logoProps={{}} />);
    expect(screen.getByTestId("page-mark-glyph")).toHaveTextContent("¶");
  });
});

describe("PageMarkButton read-only", () => {
  it("shows the glyph with no picker anywhere", () => {
    render(<PageMarkButton logoProps={EMOJI} editable={false} titleEmpty={false} onPick={vi.fn()} />);
    expect(screen.getByTestId("page-mark")).toHaveAttribute("data-readonly", "true");
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    expect(screen.getByText("🚀")).toBeInTheDocument();
  });

  it("draws nothing interactive on a page with no mark", () => {
    render(<PageMarkButton logoProps={{}} editable={false} titleEmpty onPick={vi.fn()} />);
    expect(screen.getByTestId("page-mark")).toHaveAttribute("aria-hidden", "true");
    expect(screen.queryByTestId("page-mark-affordance")).not.toBeInTheDocument();
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });
});

describe("PageMarkButton editable", () => {
  function mount(options: { titleEmpty?: boolean; logoProps?: Record<string, unknown> } = {}) {
    const onPick = vi.fn();
    const view = render(
      <PageMarkButton
        logoProps={options.logoProps ?? EMOJI}
        editable
        titleEmpty={options.titleEmpty ?? false}
        onPick={onPick}
      />
    );
    return { onPick, ...view };
  }

  it("hides the affordance until the canvas header is hovered", () => {
    mount({ logoProps: {} });
    const affordance = screen.getByTestId("page-mark-affordance");
    // The header owns the hover; the affordance is the faded child.
    expect(affordance.className).toContain("transition-all");
    expect(affordance.className).toContain("opacity-0");
    expect(affordance.className).toContain("group-hover/page-header:opacity-100");
    expect(affordance.className).toContain("group-focus-within/page-header:opacity-100");
    expect(affordance).not.toHaveAttribute("data-revealed");
  });

  it("pins the affordance while the title is empty", () => {
    mount({ logoProps: {}, titleEmpty: true });
    expect(screen.getByTestId("page-mark")).toHaveAttribute("data-pinned", "true");
    const affordance = screen.getByTestId("page-mark-affordance");
    expect(affordance.className).toContain("opacity-100");
    expect(affordance).toHaveAttribute("data-revealed", "true");
  });

  it("names the trigger for the stored mark", () => {
    mount();
    expect(screen.getByTestId("page-mark-trigger")).toHaveAccessibleName(PAGE_MARK_CHANGE_LABEL);
  });

  it("names the trigger for an unmarked page", () => {
    mount({ logoProps: {} });
    expect(screen.getByTestId("page-mark-trigger")).toHaveAccessibleName(PAGE_MARK_EMPTY_LABEL);
  });

  it("persists a picked mark", () => {
    const { onPick } = mount({ logoProps: {} });
    fireEvent.click(screen.getByTestId("page-mark-trigger"));
    fireEvent.click(screen.getByRole("tab", { name: "Emoji" }));
    fireEvent.change(screen.getByPlaceholderText("Search or type an emoji"), { target: { value: "fire" } });
    fireEvent.click(screen.getByRole("button", { name: "fire" }));
    expect(onPick).toHaveBeenCalledWith({ in_use: "emoji", emoji: { value: "128293" } });
  });

  it("persists a cleared mark", () => {
    const { onPick } = mount();
    fireEvent.click(screen.getByTestId("page-mark-trigger"));
    fireEvent.click(screen.getByRole("button", { name: "Remove mark" }));
    expect(onPick).toHaveBeenCalledWith({});
  });
});
