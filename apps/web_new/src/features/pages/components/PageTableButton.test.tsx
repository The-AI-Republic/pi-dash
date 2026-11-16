// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Table toolbar tile (parity/specs/pages.md §5.3, PAGE-033).

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { PageTableButton, PAGE_TABLE_TEST_ID } from "./PageTableButton.js";

describe("PageTableButton", () => {
  it("is one square icon tile named Table", () => {
    render(<PageTableButton onInsert={vi.fn()} />);
    const button = screen.getByRole("button", { name: "Table" });
    expect(button).toHaveAttribute("data-testid", PAGE_TABLE_TEST_ID);
    expect(button.className).toContain("size-7");
    expect(button.querySelector("svg")).not.toBeNull();
  });

  it("inserts on press and refuses mouse-down so the caret survives", () => {
    const onInsert = vi.fn();
    render(<PageTableButton onInsert={onInsert} />);
    expect(fireEvent.mouseDown(screen.getByTestId(PAGE_TABLE_TEST_ID))).toBe(false);
    fireEvent.click(screen.getByTestId(PAGE_TABLE_TEST_ID));
    expect(onInsert).toHaveBeenCalledTimes(1);
  });
});
