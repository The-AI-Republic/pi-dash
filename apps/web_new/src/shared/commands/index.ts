// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

export {
  clearCommands,
  clearPalettePages,
  filterCommands,
  filterPageOptions,
  findCommand,
  findCommandBySequence,
  findPalettePage,
  listBoundCommands,
  listCommands,
  matchesShortcut,
  parseShortcut,
  registerCommands,
  registerPalettePages,
} from "./registry.js";
export type {
  PaletteCommandContext,
  PalettePage,
  PalettePageOption,
  PalettePageState,
  PidashCommand,
} from "./registry.js";
export { formatSequenceKeys, isTypingTarget } from "./shortcuts.js";
export {
  countSearchHits,
  groupSearchResults,
  paletteSearchKeys,
  paletteSearchQueryOptions,
  useDebouncedValue,
  PALETTE_SEARCH_DEBOUNCE_MS,
} from "./search.js";
export type { PaletteSearchParams, PaletteSearchScope, SearchResultGroup, SearchResultHit } from "./search.js";
export { useCommandPalette } from "./store.js";
