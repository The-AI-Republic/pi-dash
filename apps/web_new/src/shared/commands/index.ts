// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

export {
  clearCommands,
  findCommand,
  listCommands,
  matchesShortcut,
  parseShortcut,
  registerCommands,
} from "./registry.js";
export type { PidashCommand } from "./registry.js";
export { useCommandPalette } from "./store.js";
