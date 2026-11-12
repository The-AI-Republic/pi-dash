// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Validated search for the modules list address (parity/specs/modules.md
// §5, MOD-019). The screen owns one key of its own, `peekModule`; every
// other key in the address belongs to whoever put it there and is carried
// through untouched, so opening or closing the peek never costs the viewer
// a shared link's other parameters. An unusable `peekModule` value is
// dropped at the route boundary so a malformed link opens the plain list
// instead of a panel with nothing to show.

import { MODULE_PEEK_PARAM } from "./moduleEntry.js";
import { modulePeekFromSearch } from "./modulePeek.js";

export interface ModulesListSearch {
  /** The module the peek panel shows; absent when no panel is open. */
  peekModule?: string;
  /** Address keys this screen does not interpret, preserved as they came. */
  [key: string]: unknown;
}

export function parseModulesListSearch(raw: Record<string, unknown>): ModulesListSearch {
  const { [MODULE_PEEK_PARAM]: _value, ...rest } = raw;
  const peekModule = modulePeekFromSearch(raw);
  if (peekModule === null) return rest;
  return { ...rest, [MODULE_PEEK_PARAM]: peekModule };
}
