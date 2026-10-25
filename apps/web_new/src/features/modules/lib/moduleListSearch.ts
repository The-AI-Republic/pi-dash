// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Validated search for the modules list address (parity/specs/modules.md
// §5, MOD-019). The only key this screen owns today is `peekModule`; an
// unusable value is dropped at the route boundary so a malformed shared
// link opens the plain list instead of a panel with nothing to show. The
// layout, ordering and filters are browser state, not address state, and
// stay out of the schema on purpose.

import { MODULE_PEEK_PARAM } from "./moduleEntry.js";
import { modulePeekFromSearch } from "./modulePeek.js";

export interface ModulesListSearch {
  /** The module the peek panel shows; absent when no panel is open. */
  peekModule?: string;
}

export function parseModulesListSearch(raw: Record<string, unknown>): ModulesListSearch {
  const peekModule = modulePeekFromSearch(raw);
  return peekModule === null ? {} : { [MODULE_PEEK_PARAM]: peekModule };
}
