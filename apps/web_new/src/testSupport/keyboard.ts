// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared keyboard/focus assertions for the export surfaces (EXP-023,
// NEWFRONT-773). Every export control is reachable by Tab and shows the kit's
// visible focus ring; Enter activates the control it lands on; content keys
// outside text fields change nothing. The helpers keep the same semantics on
// the settings form, the history, the analytics export actions, the profile
// download and the page-export dialog, so the cross-surface proof cannot drift
// per screen.

import { expect } from "vitest";

/** The kit's visible focus treatment (packages/kit/src/lib/styles.ts). */
export const VISIBLE_FOCUS_CLASS = "focus-visible:outline-solid";

/** Controls a keyboard user can reach on an export surface. */
export type ExportControl = HTMLElement;

function controlName(control: ExportControl): string {
  const labelledBy = control.getAttribute("aria-labelledby");
  if (labelledBy !== null) {
    const text = labelledBy
      .split(/\s+/)
      .map((id) => document.getElementById(id)?.textContent ?? "")
      .join(" ")
      .trim();
    if (text !== "") return text;
  }
  const label = control.getAttribute("aria-label");
  if (label !== null && label !== "") return label;
  return (control.textContent ?? "").trim();
}

/** True when the control participates in the tab order. */
export function isTabReachable(control: ExportControl): boolean {
  if (control.hasAttribute("disabled")) return false;
  const tabIndex = control.getAttribute("tabindex");
  if (tabIndex === null) return true;
  return Number(tabIndex) >= 0;
}

/**
 * Every control in `root` is in the tab order and carries the kit focus ring.
 * Buttons, links and native form controls only; containers and text stay out.
 */
export function expectKeyboardReachable(root: HTMLElement): ExportControl[] {
  const controls = [...root.querySelectorAll<ExportControl>("button, a[href], input, select, textarea")].filter(
    (control) => !control.hasAttribute("disabled")
  );
  expect(controls.length).toBeGreaterThan(0);
  for (const control of controls) {
    expect(isTabReachable(control), `${control.tagName} "${controlName(control)}" is not Tab-reachable`).toBe(true);
    expect(control.className, `${controlName(control)} has no visible focus ring`).toContain(VISIBLE_FOCUS_CLASS);
  }
  return controls;
}

/**
 * Tab from the active element until `control` has focus. Asserts the control
 * is reachable within the surface's own tab order and lands there. Returns
 * the labels of every control visited on the way, so a caller can also pin
 * the order it traversed.
 */
export async function tabTo(user: { tab: () => Promise<void> }, control: ExportControl, limit = 40): Promise<string[]> {
  const visited: string[] = [];
  for (let step = 0; step < limit; step += 1) {
    if (document.activeElement === control) return visited;
    await user.tab();
    const active = document.activeElement;
    if (active instanceof HTMLElement && active !== document.body && active !== control) {
      const name = controlName(active);
      if (name !== "") visited.push(name);
    }
  }
  throw new Error(
    `Tab never reached "${controlName(control)}" within ${limit} steps (visited: ${visited.join(" > ")})`
  );
}

/** Enter activates whatever control currently has focus. */
export async function pressEnter(user: { keyboard: (keys: string) => Promise<void> }): Promise<void> {
  await user.keyboard("{Enter}");
}

/** Escape dismisses the focused surface. */
export async function pressEscape(user: { keyboard: (keys: string) => Promise<void> }): Promise<void> {
  await user.keyboard("{Escape}");
}

/**
 * Content keys pressed with no text field focused change nothing. Callers
 * pass a snapshot function and assert its value is unchanged afterwards.
 */
export async function expectContentKeysInert(
  user: { keyboard: (keys: string) => Promise<void> },
  snapshot: () => string
): Promise<void> {
  const before = snapshot();
  for (const key of ["e", "x", "p", "m", "?"]) {
    await user.keyboard(key);
  }
  expect(snapshot()).toBe(before);
}
