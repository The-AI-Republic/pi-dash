// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).

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
