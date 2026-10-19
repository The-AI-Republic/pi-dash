// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Build-time platform selection. The bundler (and vitest) resolve
// @pidash/platform-target to web.ts or tauri.ts, so the web bundle never
// contains desktop code. Features import { platform } from here and never
// branch on the target themselves.

export type {
  AgentEngineProfile,
  AgentHostCheck,
  AgentRuntimeApi,
  DeepLinkApi,
  EventStream,
  KeyValueStore,
  MenuApi,
  MenuCommand,
  PendingUpdate,
  Platform,
  SessionTeardownApi,
  StreamInit,
  StreamMessage,
  TitleBarApi,
  Unsubscribe,
  UpdaterApi,
} from "./types.js";
export { platform } from "@pidash/platform-target";
export { toApiTransport } from "./transport.js";
export { isWebAddress } from "./urls.js";
export { createIndexedDbStore, createLocalStorageStore, createMemoryStore, createPersistentStore } from "./storage.js";
