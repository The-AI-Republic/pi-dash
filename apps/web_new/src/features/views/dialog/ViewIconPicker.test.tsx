// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { VIEW_ICON_COLORS, ViewIconPicker, ViewLogoMark, type ViewLogoProps } from "./ViewIconPicker.js";

function renderPicker(value: ViewLogoProps | null = null, disabled = false): { onChange: ReturnType<typeof vi.fn> } {
  const onChange = vi.fn();
  render(<ViewIconPicker value={value} onChange={onChange} disabled={disabled} />);
  return { onChange };
}

describe("ViewLogoMark", () => {
  it("renders the default glyph without a logo", () => {
    const { container } = render(<ViewLogoMark logo={null} />);
    expect(container.querySelector("svg")).not.toBeNull();
  });

  it("renders the picked emoji", () => {
    render(<ViewLogoMark logo={{ in_use: "emoji", emoji: { value: "🚀" } }} />);
    expect(screen.getByText("🚀")).toBeInTheDocument();
  });

  it("renders the picked glyph in its color", () => {
    const { container } = render(<ViewLogoMark logo={{ in_use: "icon", icon: { name: "Flag", color: "#c0362c" } }} />);
    expect(container.querySelector('svg[stroke="#c0362c"]')).not.toBeNull();
  });

  it("falls back to the default glyph for an unknown name", () => {
    const { container } = render(<ViewLogoMark logo={{ in_use: "icon", icon: { name: "Nope", color: "#000000" } }} />);
    expect(container.querySelector("svg")).not.toBeNull();
  });
});

describe("ViewIconPicker", () => {
  it("shows the current logo on its trigger", () => {
    renderPicker({ in_use: "emoji", emoji: { value: "🎯" } });
    expect(within(screen.getByTestId("view-icon-picker")).getByText("🎯")).toBeInTheDocument();
  });

  it("offers Emoji and Icon tabs and picks an emoji", async () => {
    const user = userEvent.setup();
    const { onChange } = renderPicker();
    await user.click(screen.getByTestId("view-icon-picker"));
    const tabs = screen.getAllByRole("tab");
    expect(tabs.map((tab) => tab.textContent)).toEqual(["Emoji", "Icon"]);
    await user.click(within(screen.getByTestId("view-icon-emojis")).getByRole("button", { name: "Emoji 🚀" }));
    expect(onChange).toHaveBeenCalledWith({ in_use: "emoji", emoji: { value: "🚀" } });
    expect(screen.queryByTestId("view-icon-emojis")).not.toBeInTheDocument();
  });

  it("picks a glyph with the default swatch color", async () => {
    const user = userEvent.setup();
    const { onChange } = renderPicker();
    await user.click(screen.getByTestId("view-icon-picker"));
    await user.click(screen.getByRole("tab", { name: "Icon" }));
    await user.click(within(screen.getByTestId("view-icon-glyphs")).getByRole("button", { name: "View icon Flag" }));
    expect(onChange).toHaveBeenCalledWith({
      in_use: "icon",
      icon: { name: "Flag", color: VIEW_ICON_COLORS[0] },
    });
    expect(screen.queryByTestId("view-icon-glyphs")).not.toBeInTheDocument();
  });

  it("picks a glyph with a chosen swatch color", async () => {
    const user = userEvent.setup();
    const { onChange } = renderPicker();
    await user.click(screen.getByTestId("view-icon-picker"));
    await user.click(screen.getByRole("tab", { name: "Icon" }));
    await user.click(screen.getByRole("radio", { name: "Icon color #c0362c" }));
    await user.click(within(screen.getByTestId("view-icon-glyphs")).getByRole("button", { name: "View icon Star" }));
    expect(onChange).toHaveBeenCalledWith({ in_use: "icon", icon: { name: "Star", color: "#c0362c" } });
  });

  it("takes no edits while disabled", async () => {
    const user = userEvent.setup();
    renderPicker(null, true);
    expect(screen.getByTestId("view-icon-picker")).toBeDisabled();
    await user.click(screen.getByTestId("view-icon-picker"));
    expect(screen.queryByRole("tablist", { name: "Icon type" })).not.toBeInTheDocument();
  });
});
