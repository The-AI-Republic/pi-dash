// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Heading preview (parity/specs/pages.md §6.5, PAGE-046): one bar per
// heading at the canvas edge, the heading card that hover reveals, and the
// selection that opens the outline.

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { PAGE_PREVIEW_OPEN_LABEL, PageHeadingPreview } from "./PageHeadingPreview.js";
import { PAGE_PREVIEW_BAR_WIDTH_PX, type PageHeading } from "../lib/pagePane.js";

const HEADINGS: PageHeading[] = [
  { level: 1, text: "Top", levelIndex: 1 },
  { level: 2, text: "", levelIndex: 1 },
  { level: 3, text: "Low", levelIndex: 1 },
];

describe("PageHeadingPreview", () => {
  it("renders nothing for a document with no headings", () => {
    const { container } = render(<PageHeadingPreview headings={[]} onOpen={vi.fn()} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("draws one sized bar per heading", () => {
    render(<PageHeadingPreview headings={HEADINGS} onOpen={vi.fn()} />);
    const button = screen.getByRole("button", { name: PAGE_PREVIEW_OPEN_LABEL });
    const bars = Array.from(button.querySelectorAll<HTMLElement>('div[style*="width"]'));
    expect(bars.map((bar) => bar.style.width)).toEqual(
      HEADINGS.map((heading) => `${PAGE_PREVIEW_BAR_WIDTH_PX[heading.level]}px`)
    );
  });

  it("lists the heading texts in a card that fades in on hover", () => {
    render(<PageHeadingPreview headings={HEADINGS} onOpen={vi.fn()} />);
    const button = screen.getByRole("button", { name: PAGE_PREVIEW_OPEN_LABEL });
    const card = button.parentElement?.querySelector(':scope > div:not([role="button"])');
    expect(card).not.toBeNull();
    // Hidden with opacity rather than display: the card stays sized.
    expect(card?.className).toContain("opacity-0");
    expect(card?.className).toContain("group-hover:opacity-100");
    const buttons = Array.from(card?.querySelectorAll("button") ?? []);
    expect(buttons.map((entry) => entry.textContent)).toEqual(["Top", "Untitled heading", "Low"]);
    expect(buttons.map((entry) => (entry as HTMLElement).style.paddingLeft)).toEqual(["8px", "16px", "24px"]);
  });

  it("opens the outline from the bar strip", () => {
    const onOpen = vi.fn();
    render(<PageHeadingPreview headings={HEADINGS} onOpen={onOpen} />);
    fireEvent.click(screen.getByRole("button", { name: PAGE_PREVIEW_OPEN_LABEL }));
    expect(onOpen).toHaveBeenCalledWith(null);
  });

  it("opens the outline at the picked heading", () => {
    const onOpen = vi.fn();
    render(<PageHeadingPreview headings={HEADINGS} onOpen={onOpen} />);
    fireEvent.click(screen.getByRole("button", { name: "Low" }));
    expect(onOpen).toHaveBeenCalledWith(HEADINGS[2]);
  });
});
