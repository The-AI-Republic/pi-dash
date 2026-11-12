// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared stacking tier for portalled kit overlays (NEWFRONT-950, ISS-172).
// A popup portals to the body outside the page tree, so with `z-index:
// auto` any sticky or fixed screen layer (a `z-10..60` row, the peek
// panel) paints over it and swallows its clicks while the popup still
// reads as visible. Every overlay below lands its portalled element on
// the one popover tier above those layers instead.
import { fireEvent, render, screen } from "@testing-library/react";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it, vi } from "vitest";
import { POPOVER_LAYER_CLASS } from "../lib/styles";
import { Button } from "./Button";
import { Combobox } from "./Combobox";
import { ContextMenu } from "./ContextMenu";
import { Dialog } from "./Dialog";
import { Menu } from "./Menu";
import { Popover } from "./Popover";
import { Tooltip } from "./Tooltip";

/**
 * The Base UI positioner above overlay content: the toolkit renders it
 * with `role="presentation"`, and it is the element that lives in the
 * portal, so it is the one that must carry the stacking tier.
 */
function positionerOf(node: Element): HTMLElement {
  const positioner = node.closest('[role="presentation"]');
  if (positioner instanceof HTMLElement) return positioner;
  throw new Error("no popup positioner above this overlay content");
}

describe("popover layer", () => {
  it("lands popover popups on the tier above in-page chrome", () => {
    render(
      <Popover trigger={<Button>Details</Button>}>
        <p>Popover body</p>
      </Popover>
    );
    fireEvent.click(screen.getByRole("button", { name: "Details" }));
    expect(positionerOf(screen.getByText("Popover body")).className).toContain(POPOVER_LAYER_CLASS);
  });

  it("lands combobox lists on the tier above in-page chrome", () => {
    const onSelect = vi.fn();
    render(
      <Combobox
        trigger={<Button>Priority</Button>}
        options={[
          { id: "high", label: "High" },
          { id: "low", label: "Low" },
        ]}
        value={null}
        onSelect={onSelect}
        listLabel="Priorities"
        emptyText="No matching results"
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "Priority" }));
    const listbox = screen.getByRole("listbox", { name: "Priorities" });
    expect(positionerOf(listbox).className).toContain(POPOVER_LAYER_CLASS);
    // The tier only matters because picks must land: the option still
    // reports through the same open popup.
    fireEvent.click(screen.getByRole("option", { name: "High" }));
    expect(onSelect).toHaveBeenCalledWith("high");
  });

  it("lands menus on the tier above in-page chrome", () => {
    // fireEvent dispatches the real click path (see the Menu suite for
    // why mouse behavior avoids user-event's back-to-back sequence).
    render(<Menu trigger={<Button>Actions</Button>} items={[{ label: "Edit", onSelect: vi.fn() }]} />);
    fireEvent.click(screen.getByRole("button", { name: "Actions" }));
    expect(positionerOf(screen.getByRole("menu", { name: "Actions" })).className).toContain(POPOVER_LAYER_CLASS);
  });

  it("lands context menus on the tier above in-page chrome", () => {
    render(<ContextMenu trigger={<span>Tab</span>} items={[{ label: "Set as default", onSelect: vi.fn() }]} />);
    fireEvent.contextMenu(screen.getByText("Tab"));
    expect(positionerOf(screen.getByRole("menu")).className).toContain(POPOVER_LAYER_CLASS);
  });

  it("lands tooltips on the tier above in-page chrome", () => {
    // Synthetic hover never starts the timer-driven open delay under
    // jsdom (see the overlays suite), so the open state is driven
    // directly while the layering is asserted for real.
    render(
      <Tooltip content="Create issue" open onOpenChange={vi.fn()}>
        <Button>New</Button>
      </Tooltip>
    );
    expect(positionerOf(screen.getByRole("tooltip")).className).toContain(POPOVER_LAYER_CLASS);
  });

  it("lands dialog backdrops and popups on the tier above in-page chrome", () => {
    // A dialog has no positioner: the backdrop and the popup are the
    // portalled elements, so both carry the tier (the kit renders the
    // backdrop directly before the popup).
    render(
      <Dialog trigger={<Button>Edit title</Button>} title="Edit title">
        <p>Dialog body</p>
      </Dialog>
    );
    fireEvent.click(screen.getByRole("button", { name: "Edit title" }));
    const popup = screen.getByRole("dialog", { name: "Edit title" });
    expect(popup.className).toContain(POPOVER_LAYER_CLASS);
    // The portal also holds Base UI's inert veil and focus guards, so the
    // backdrop is found by the kit's own scrim classes, not by position.
    const backdrop = Array.from(popup.parentElement?.children ?? []).find(
      (node) => node instanceof HTMLElement && node.className.includes("bg-black/40")
    );
    expect(backdrop?.className).toContain(POPOVER_LAYER_CLASS);
  });

  it("wires the tier class through the shipped utility to the token", () => {
    // The suite runs with the package directory as cwd
    // (pnpm --filter @pidash/kit test), so the shipped files are read
    // directly: the constant names a utility kit.css defines, and the
    // utility reads the token tokens.css ships.
    expect(POPOVER_LAYER_CLASS).toBe("layer-popover");
    const utilities = readFileSync(join(process.cwd(), "src", "kit.css"), "utf8");
    expect(utilities).toContain("@utility layer-popover");
    expect(utilities).toMatch(/@utility layer-popover\s*\{[^}]*z-index:\s*var\(--layer-popover\)[^}]*\}/);
    const tokens = readFileSync(join(process.cwd(), "src", "tokens.css"), "utf8");
    expect(tokens).toMatch(/--layer-popover:\s*70\s*;/);
  });
});
