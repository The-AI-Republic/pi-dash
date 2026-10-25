// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Outline tab (parity/specs/pages.md §6.6, PAGE-047): headings in order
// indented by level, click-to-scroll, and the themed prompt for a document
// without headings.

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

const scheme = vi.hoisted(() => ({ current: "light" as string, customDark: false }));
vi.mock("../../../core/theme/index.js", () => ({
  useResolvedScheme: () => ({ scheme: scheme.current, customDark: scheme.customDark }),
}));

import { PAGE_OUTLINE_EMPTY_ART_ALT, PAGE_OUTLINE_EMPTY_TITLE, PageOutlinePanel } from "./PageOutlinePanel.js";
import { PAGE_OUTLINE_INDENT_PX, type PageHeading } from "../lib/pagePane.js";

const HEADINGS: PageHeading[] = [
  { level: 1, text: "Top", levelIndex: 1 },
  { level: 2, text: "Middle", levelIndex: 1 },
  { level: 3, text: "", levelIndex: 1 },
];

describe("PageOutlinePanel", () => {
  it("shows the themed prompt and its art when there are no headings", () => {
    render(<PageOutlinePanel headings={[]} onSelectHeading={vi.fn()} />);
    expect(screen.getByText(PAGE_OUTLINE_EMPTY_TITLE)).toBeInTheDocument();
    expect(screen.getByAltText(PAGE_OUTLINE_EMPTY_ART_ALT)).toBeInTheDocument();
    expect(screen.queryByTestId("page-outline")).not.toBeInTheDocument();
  });

  it("follows the theme's illustration for the prompt", () => {
    scheme.current = "custom";
    scheme.customDark = true;
    try {
      render(<PageOutlinePanel headings={[]} onSelectHeading={vi.fn()} />);
      expect(screen.getByAltText(PAGE_OUTLINE_EMPTY_ART_ALT).getAttribute("src")).toContain("empty-dark.svg");
    } finally {
      scheme.current = "light";
      scheme.customDark = false;
    }
  });

  it("indents the first three levels 1:2:4 and reports a pick", () => {
    const onSelectHeading = vi.fn();
    render(<PageOutlinePanel headings={HEADINGS} onSelectHeading={onSelectHeading} />);
    const buttons = screen.getAllByRole("button");
    expect(buttons.map((button) => button.style.paddingLeft)).toEqual([
      `${PAGE_OUTLINE_INDENT_PX[1]}px`,
      `${PAGE_OUTLINE_INDENT_PX[2]}px`,
      `${PAGE_OUTLINE_INDENT_PX[3]}px`,
    ]);
    expect(buttons.map((button) => button.textContent)).toEqual(["Top", "Middle", "Untitled heading"]);
    fireEvent.click(buttons[1]!);
    expect(onSelectHeading).toHaveBeenCalledWith(HEADINGS[1]);
  });
});
