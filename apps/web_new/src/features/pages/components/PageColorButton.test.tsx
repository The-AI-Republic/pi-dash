// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Letter/wash colour control (parity/specs/pages.md §5.3, PAGE-036): the
// trigger previews the current pair, the popover offers one row per kind
// with eight swatches and a clear, and a pick or clear fires for that kind.

import { fireEvent, render, screen, within } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { PAGE_COLOR_CHOICES } from "../lib/pageColors.js";
import {
  PageColorButton,
  PAGE_COLOR_PANEL_TEST_ID,
  PAGE_COLOR_ROW_TEST_ID,
  PAGE_COLOR_TEST_ID,
} from "./PageColorButton.js";

function mount(props: Partial<React.ComponentProps<typeof PageColorButton>> = {}) {
  const onPick = vi.fn();
  const onClear = vi.fn();
  render(
    <PageColorButton
      textColor={props.textColor ?? null}
      backgroundColor={props.backgroundColor ?? null}
      onPick={props.onPick ?? onPick}
      onClear={props.onClear ?? onClear}
    />
  );
  return { onPick, onClear };
}

function open(): HTMLElement {
  fireEvent.click(screen.getByTestId(PAGE_COLOR_TEST_ID));
  return screen.getByTestId(PAGE_COLOR_PANEL_TEST_ID);
}

describe("PageColorButton trigger", () => {
  it("is a Color control whose preview starts unpainted", () => {
    mount();
    const trigger = screen.getByTestId(PAGE_COLOR_TEST_ID);
    expect(trigger).toHaveAccessibleName("Color");
    expect(trigger).toHaveAttribute("aria-expanded", "false");
    const preview = within(trigger).getByTestId("page-color-preview");
    expect(preview.style.backgroundColor).toBe("transparent");
    expect(preview.querySelector("svg")?.getAttribute("style") ?? "").toContain("color: inherit");
  });

  it("previews the stored letter and wash paints", () => {
    mount({ textColor: "pink", backgroundColor: "green" });
    const preview = within(screen.getByTestId(PAGE_COLOR_TEST_ID)).getByTestId("page-color-preview");
    expect(preview.style.backgroundColor).toBe("var(--page-color-green-wash)");
    expect(preview.querySelector("svg")?.getAttribute("style")).toContain("var(--page-color-pink-text)");
  });

  it("refuses mouse-down so the editor keeps its selection", () => {
    mount();
    expect(fireEvent.mouseDown(screen.getByTestId(PAGE_COLOR_TEST_ID))).toBe(false);
  });

  it("reports itself expanded once open", () => {
    mount();
    open();
    expect(screen.getByTestId(PAGE_COLOR_TEST_ID)).toHaveAttribute("aria-expanded", "true");
  });
});

describe("PageColorButton popover", () => {
  it("offers a letter row then a wash row", () => {
    mount();
    open();
    const text = screen.getByTestId(`${PAGE_COLOR_ROW_TEST_ID}-text`);
    const background = screen.getByTestId(`${PAGE_COLOR_ROW_TEST_ID}-background`);
    expect(text).toHaveAttribute("aria-label", "Letter colors");
    expect(background).toHaveAttribute("aria-label", "Wash colors");
    const rows = Array.from(screen.getByTestId(PAGE_COLOR_PANEL_TEST_ID).children);
    expect(rows[0]).toBe(text);
    expect(rows[1]).toBe(background);
  });

  it("draws eight swatches and one clear per row", () => {
    mount();
    open();
    for (const kind of ["text", "background"] as const) {
      const row = screen.getByTestId(`${PAGE_COLOR_ROW_TEST_ID}-${kind}`);
      const buttons = within(row).getAllByRole("button");
      expect(buttons).toHaveLength(PAGE_COLOR_CHOICES.length + 1);
      expect(
        within(row).getByRole("button", { name: kind === "text" ? "Clear letter color" : "Clear wash color" })
      ).toBeInTheDocument();
      for (const choice of PAGE_COLOR_CHOICES) {
        expect(
          within(row).getByRole("button", {
            name: kind === "text" ? `Letter color ${choice.label}` : `Wash color ${choice.label}`,
          })
        ).toBeInTheDocument();
      }
    }
  });

  it("paints each swatch with its kind's token and marks the stored one", () => {
    mount({ textColor: "pink" });
    open();
    const pink = screen.getByTestId("page-color-swatch-text-pink");
    expect(pink.style.backgroundColor).toBe("var(--page-color-pink-text)");
    expect(pink).toHaveAttribute("aria-pressed", "true");
    const gray = screen.getByTestId("page-color-swatch-text-gray");
    expect(gray.style.backgroundColor).toBe("var(--page-color-gray-text)");
    expect(gray).toHaveAttribute("aria-pressed", "false");
  });

  it("does not light a wash swatch for a matching letter key", () => {
    mount({ textColor: "pink" });
    open();
    expect(screen.getByTestId("page-color-swatch-background-pink")).toHaveAttribute("aria-pressed", "false");
  });

  it("applies a swatch for its own kind and closes", () => {
    const { onPick, onClear } = mount();
    open();
    fireEvent.click(screen.getByTestId("page-color-swatch-background-orange"));
    expect(onPick).toHaveBeenCalledWith("background", "orange");
    expect(onClear).not.toHaveBeenCalled();
    expect(screen.queryByTestId(PAGE_COLOR_PANEL_TEST_ID)).not.toBeInTheDocument();
  });

  it("clears the row's kind and closes", () => {
    const { onPick, onClear } = mount();
    open();
    fireEvent.click(screen.getByTestId("page-color-clear-text"));
    expect(onClear).toHaveBeenCalledWith("text");
    expect(onPick).not.toHaveBeenCalled();
    expect(screen.queryByTestId(PAGE_COLOR_PANEL_TEST_ID)).not.toBeInTheDocument();
  });

  it("keeps the selection on a swatch press", () => {
    mount();
    open();
    expect(fireEvent.mouseDown(screen.getByTestId("page-color-swatch-text-gray"))).toBe(false);
    expect(fireEvent.mouseDown(screen.getByTestId("page-color-clear-background"))).toBe(false);
  });
});
