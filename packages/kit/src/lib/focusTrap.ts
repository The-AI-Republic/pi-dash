// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Synchronous wrap point for a modal's Tab sequence. Base UI's own trap
// answers a Tab past the last control by focusing a focus guard and
// handing focus back a frame later, and it leaves the page behind only
// `aria-hidden` (still tabbable). A user who holds Tab, or presses it
// before that frame lands, therefore steps through the guards into the
// sidebar and topbar. Wrapping inside the popup itself keeps the cycle
// synchronous, so the next Tab can never leave the dialog.

const TABBABLE_SELECTOR = [
  "a[href]",
  "area[href]",
  "button",
  "input",
  "select",
  "textarea",
  "summary",
  "audio[controls]",
  "video[controls]",
  "[contenteditable='true']",
  "[tabindex]",
].join(",");

/** Whether the element still takes a place in the Tab sequence. */
function isTabbable(element: HTMLElement): boolean {
  if (element.hasAttribute("disabled")) return false;
  const tabIndex = element.getAttribute("tabindex");
  if (tabIndex !== null && Number(tabIndex) < 0) return false;
  // aria-hidden alone does not remove a control from the Tab sequence
  // (the background behind a Base UI dialog is only aria-hidden and stays
  // reachable); only the inert attribute does.
  if (element instanceof HTMLInputElement && element.type === "hidden") return false;
  if (element.closest("[inert]") !== null) return false;
  // jsdom has no checkVisibility; the structural checks above still hold.
  if (typeof element.checkVisibility === "function" && !element.checkVisibility()) return false;
  return true;
}

/** Every control the container's own Tab sequence visits, in DOM order. */
export function tabbableWithin(container: HTMLElement | null): HTMLElement[] {
  if (container === null) return [];
  return Array.from(container.querySelectorAll<HTMLElement>(TABBABLE_SELECTOR)).filter(isTabbable);
}

/**
 * The stop a Tab press at `active` should land on instead of leaving the
 * container: the last control for a shift-Tab at the top, the first for a
 * forward Tab at the bottom, and null while the browser's own move stays
 * inside. Focus that already sits outside the container is left alone —
 * a nested overlay (the project picker's popup) owns its own Tab, and the
 * caret on the container itself moves to the first stop on its own. A
 * container with no stops also returns null so the caller keeps the
 * browser's behavior.
 */
export function wrapTabTarget(
  container: HTMLElement | null,
  active: Element | null,
  shiftKey: boolean
): HTMLElement | null {
  if (container === null || active === null || !container.contains(active)) return null;
  const stops = tabbableWithin(container);
  const first = stops[0];
  const last = stops[stops.length - 1];
  if (first === undefined || last === undefined) return null;
  if (shiftKey) return active === first ? last : null;
  return active === last ? first : null;
}
