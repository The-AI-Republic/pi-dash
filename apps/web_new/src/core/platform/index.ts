// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Build-time platform selection. The bundler (and vitest) resolve
// @pidash/platform-target to web.ts or tauri.ts, so the web bundle never
// contains desktop code. Features import { platform } from here and never
// branch on the target themselves.

export type {
  AgentRuntimeApi,
  DeepLinkApi,
  EventStream,
  KeyValueStore,
  MenuApi,
  MenuCommand,
  Platform,
  StreamInit,
  StreamMessage,
  TitleBarApi,
  Unsubscribe,
  UpdaterApi,
} from "./types.js";
export { platform } from "@pidash/platform-target";
export { toApiTransport } from "./transport.js";
export { createIndexedDbStore, createLocalStorageStore, createMemoryStore, createPersistentStore } from "./storage.js";
