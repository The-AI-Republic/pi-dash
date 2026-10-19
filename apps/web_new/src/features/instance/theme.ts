// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Boot theme for the instance screens (parity/specs/workspace-settings.md
// §34, WSET-034). The instance gate renders above the router's theme
// provider, so it must mark the document itself from the stored choice;
// otherwise a dark-preferring user would see the light maintenance art
// and light tokens until the router mounted.

import {
  applyResolvedTheme,
  prefersDarkScheme,
  readStoredTheme,
  resolveScheme,
  type ResolvedScheme,
} from "../../core/theme/index.js";

/** Apply the stored theme mark to the document (or a test target). */
export function markInstanceBootScheme(target: HTMLElement = document.documentElement): ResolvedScheme {
  const scheme = resolveScheme(readStoredTheme(), prefersDarkScheme());
  applyResolvedTheme(scheme, undefined, target);
  return scheme;
}
