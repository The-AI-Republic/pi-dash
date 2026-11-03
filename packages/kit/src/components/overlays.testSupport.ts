// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
/**
 * The portalled positioner above a Base UI popup: the element that carries
 * the popup tier. It walks up from any node inside the popup (a popup can
 * nest its own list), so tests pin the layering without depending on the
 * toolkit's private attribute names.
 */
export function positionerOf(popupPart: Element): HTMLElement {
  for (let node = popupPart.parentElement; node !== null; node = node.parentElement) {
    if (node.getAttribute("role") === "presentation") return node;
  }
  throw new Error("no popup positioner above this popup content");
}
