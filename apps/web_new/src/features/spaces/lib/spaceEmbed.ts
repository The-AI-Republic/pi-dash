// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Embedded-frame signal (parity/specs/space.md SPC-015). One rule with
// no heuristics: the board is embedded when its window is not the top
// window. The account slot, the product badge and the emoji add controls
// read this and render nothing inside a frame, while the board itself,
// reaction counts and vote counts stay readable. Later slices (overlay,
// engagement, comments) import the same signal rather than re-checking.
// Frame membership never changes during a page's life, so the hook is a
// single read, settled on the first paint with no flash of controls.
import * as React from "react";

export interface FramedWindow {
  self: unknown;
  top: unknown;
}

/** True when the given window sits inside another window's frame. */
export function isEmbeddedWindow(windowLike: FramedWindow | undefined): boolean {
  if (windowLike === undefined) return false;
  return windowLike.self !== windowLike.top;
}

/** True when this page is rendered inside an embedding frame. */
export function readSpaceEmbedded(): boolean {
  if (typeof globalThis.window === "undefined") return false;
  return isEmbeddedWindow(globalThis.window);
}

/** Shared embed signal for the space subtree. */
export function useSpaceEmbedded(): boolean {
  const [embedded] = React.useState(readSpaceEmbedded);
  return embedded;
}
