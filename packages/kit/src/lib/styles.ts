// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared class fragments for kit components. Kept as literal strings in a
// .ts module so the Tailwind scanner and every component stay in sync.

/** Always-visible 2px accent focus ring (H-visual behavior rule). */
export const FOCUS_RING =
  "focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-(--focus-ring-color) focus-visible:outline-offset-1";

/**
 * Stacking for a portalled popup element (the Base UI positioner). The
 * popup must clear every stack level a screen uses for its own sticky rows
 * and panels, or those layers paint over it and swallow its clicks; the
 * utility and its tier live in `kit.css` and `tokens.css`.
 */
export const POPOVER_LAYER_CLASS = "layer-popover";
