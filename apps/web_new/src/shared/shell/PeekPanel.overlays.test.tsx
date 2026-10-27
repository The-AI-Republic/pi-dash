// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Overlays opened inside the peek must paint above the peek panel
// (NEWFRONT-950, ISS-172). The panel is fixed at z-40..60 while kit
// popups portal to the body, so without the shared popover tier the
// panel covers every picker and menu opened inside it: options read as
// visible yet every click lands on peek content instead. jsdom has no
// hit-testing, so these tests pin the tier relationship the browser
// paints from: every overlay's portalled element carries the popover
// tier, and every peek stack level stays below it.
import { fireEvent, render, screen } from "@testing-library/react";
import { Button, Combobox, Dialog, Menu } from "@pidash/kit";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { FULL_SCREEN_PORTAL_ID } from "./document.js";
import { PeekPanel, type PeekLayout } from "./PeekPanel.js";

// The stacking contract lives in the stylesheet the kit ships (see the
// marketing tokens suite): the tier number is read here, never copied.
const kitTokens = readFileSync(resolve(process.cwd(), "../../packages/kit/src/tokens.css"), "utf8");
const popoverTier = Number(kitTokens.match(/--layer-popover:\s*(\d+)/)?.[1]);

/** The kit's popover-tier utility (packages/kit/src/lib/styles.ts). */
const POPOVER_TIER_UTILITY = "layer-popover";

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

/** Every z level a class list sets, through `z-N` and `z-[N]` spellings. */
function zLevelsOf(className: string): number[] {
  // No trailing boundary: `z-[60]` ends in a bracket, which has no word
  // edge against the class list around it.
  return [...className.matchAll(/\bz-(?:\[(\d+)\]|(\d+))/g)].map((match) => Number(match[1] ?? match[2]));
}

function setupPeek(children: React.ReactNode, props?: Partial<React.ComponentProps<typeof PeekPanel>>) {
  document.body.innerHTML = `<div id="${FULL_SCREEN_PORTAL_ID}"></div>`;
  return render(
    <PeekPanel title="Parity first issue" titleLabel="Work item title" onClose={vi.fn()} {...props}>
      {children}
    </PeekPanel>
  );
}

const PANEL_STACKS: Array<[PeekLayout, boolean]> = [
  ["side", false],
  ["side", true],
  ["modal", false],
  ["modal", true],
  ["full", false],
  ["full", true],
];

describe("peek overlay stacking", () => {
  it.each(PANEL_STACKS)("keeps the %s panel (stacked: %s) below the popover tier", (layout, stacked) => {
    setupPeek(<p>Preview body</p>, { layout, stacked });
    // The panel and (for modal) its backdrop are the peek's own stack
    // levels; every one of them must clear the tier popups share, or the
    // panel paints over any picker or menu opened inside it.
    const panel = screen.getByTestId("issue-preview");
    const levels = zLevelsOf(panel.className);
    const backdrop = screen.queryByTestId("peek-backdrop");
    if (backdrop !== null) levels.push(...zLevelsOf(backdrop.className));
    expect(levels.length).toBeGreaterThan(0);
    for (const level of levels) expect(level).toBeLessThan(popoverTier);
  });

  it("lands a picker opened inside the peek on the popover tier (ISS-172)", () => {
    // The sidebar pickers are kit comboboxes; Priority is the leg the
    // gate caught, and the rest share the component.
    const onSelect = vi.fn();
    setupPeek(
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
    expect(positionerOf(listbox).className).toContain(POPOVER_TIER_UTILITY);
    fireEvent.click(screen.getByRole("option", { name: "High" }));
    expect(onSelect).toHaveBeenCalledWith("high");
  });

  it("lands a menu opened inside the peek on the popover tier (ISS-166)", () => {
    // The display-mode switch is a kit menu in the peek header; the
    // quick-action menus open through the same component.
    const onSelect = vi.fn();
    setupPeek(
      <Menu
        trigger={<Button>Display mode</Button>}
        items={[
          { label: "Side peek", onSelect },
          { label: "Modal", onSelect },
        ]}
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "Display mode" }));
    expect(positionerOf(screen.getByRole("menu", { name: "Display mode" })).className).toContain(POPOVER_TIER_UTILITY);
    fireEvent.click(screen.getByRole("menuitem", { name: "Modal" }));
    expect(onSelect).toHaveBeenCalledTimes(1);
  });

  it("lands a dialog opened inside the peek on the popover tier", () => {
    // The parent picker is a kit dialog in the sidebar; it has no
    // positioner, so its backdrop and popup carry the tier directly.
    setupPeek(
      <Dialog trigger={<Button>Set parent</Button>} title="Set parent">
        <p>Parent search</p>
      </Dialog>
    );
    fireEvent.click(screen.getByRole("button", { name: "Set parent" }));
    const popup = screen.getByRole("dialog", { name: "Set parent" });
    expect(popup.className).toContain(POPOVER_TIER_UTILITY);
    // The portal also holds Base UI's inert veil and focus guards, so the
    // backdrop is found by the kit's own scrim classes, not by position.
    const backdrop = Array.from(popup.parentElement?.children ?? []).find(
      (node) => node instanceof HTMLElement && node.className.includes("bg-black/40")
    );
    expect(backdrop?.className).toContain(POPOVER_TIER_UTILITY);
  });
});
