// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Calendar hover-preview placement (parity/specs/issues.md §4, ISS-025).
// The card opens below the block when there is room, otherwise above it;
// it aligns to the block's start unless that would overflow the viewport.

export type PreviewPlacement = "bottom-start" | "bottom-end" | "top-start" | "top-end";

export interface AnchorRect {
  top: number;
  bottom: number;
  left: number;
  right: number;
}

export interface ViewportSize {
  width: number;
  height: number;
}

/** Estimated card size used to decide whether it fits beside the anchor. */
export const PREVIEW_WIDTH_PX = 288;
export const PREVIEW_HEIGHT_PX = 96;

/** Placement for a preview card anchored to one block. */
export function previewPlacement(
  anchor: AnchorRect,
  viewport: ViewportSize,
  card: { width: number; height: number } = { width: PREVIEW_WIDTH_PX, height: PREVIEW_HEIGHT_PX }
): PreviewPlacement {
  const side = anchor.bottom + card.height <= viewport.height ? "bottom" : "top";
  const align = anchor.left + card.width <= viewport.width ? "start" : "end";
  return `${side}-${align}` as PreviewPlacement;
}

/** Classes that place the preview card for a computed placement. */
export function previewPlacementClasses(placement: PreviewPlacement): string {
  const vertical = placement.startsWith("bottom") ? "top-full" : "bottom-full";
  const horizontal = placement.endsWith("start") ? "left-0" : "right-0";
  return `${vertical} ${horizontal}`;
}
