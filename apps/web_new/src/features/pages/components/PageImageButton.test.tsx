// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Image toolbar tile (parity/specs/pages.md §5.3, PAGE-034): the tile opens
// the file picker, hands one chosen file to the caller and refuses a second
// pick while an upload is in flight.

import { fireEvent, render, screen, within } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { PageImageButton, PAGE_IMAGE_INPUT_TEST_ID, PAGE_IMAGE_TEST_ID } from "./PageImageButton.js";

function png(): File {
  return new File(["bytes"], "shot.png", { type: "image/png" });
}

describe("PageImageButton", () => {
  it("is one square icon tile named Image with a hidden image input", () => {
    render(<PageImageButton onPick={vi.fn()} />);
    const button = screen.getByRole("button", { name: "Image" });
    expect(button).toHaveAttribute("data-testid", PAGE_IMAGE_TEST_ID);
    expect(button.className).toContain("size-7");
    expect(button.querySelector("svg")).not.toBeNull();
    const input = screen.getByTestId(PAGE_IMAGE_INPUT_TEST_ID);
    expect(input).toHaveAttribute("type", "file");
    expect(input).toHaveAttribute("accept", "image/*");
  });

  it("opens the picker on press without stealing the caret", () => {
    render(<PageImageButton onPick={vi.fn()} />);
    const input = screen.getByTestId(PAGE_IMAGE_INPUT_TEST_ID);
    const click = vi.spyOn(input, "click").mockImplementation(() => undefined);
    expect(fireEvent.mouseDown(screen.getByTestId(PAGE_IMAGE_TEST_ID))).toBe(false);
    fireEvent.click(screen.getByTestId(PAGE_IMAGE_TEST_ID));
    expect(click).toHaveBeenCalledTimes(1);
  });

  it("hands the chosen file over and clears the input for the next pick", () => {
    const onPick = vi.fn();
    render(<PageImageButton onPick={onPick} />);
    const input = screen.getByTestId(PAGE_IMAGE_INPUT_TEST_ID) as HTMLInputElement;
    const file = png();
    fireEvent.change(input, { target: { files: [file] } });
    expect(onPick).toHaveBeenCalledWith(file);
    expect(input.value).toBe("");
  });

  it("ignores a change that carries no file", () => {
    const onPick = vi.fn();
    render(<PageImageButton onPick={onPick} />);
    fireEvent.change(screen.getByTestId(PAGE_IMAGE_INPUT_TEST_ID), { target: { files: [] } });
    expect(onPick).not.toHaveBeenCalled();
  });

  it("shows a busy spinner and refuses input while uploading", () => {
    render(<PageImageButton onPick={vi.fn()} busy />);
    const button = screen.getByRole("button", { name: "Image" });
    expect(button).toBeDisabled();
    expect(button).toHaveAttribute("aria-busy", "true");
    // The busy tile paints the spinner rather than the add-picture icon.
    expect(within(button).getByRole("status")).toBeInTheDocument();
    expect(within(button).getByText("Uploading image")).toHaveClass("sr-only");
    expect(button.querySelector("svg.lucide-image-plus")).toBeNull();
  });
});
