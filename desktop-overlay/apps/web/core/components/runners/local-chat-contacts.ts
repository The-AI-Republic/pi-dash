/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * Desktop override of the runner-picker local-contacts seam
 * (PDASHOSS01-159). Adds the bundled built-in agent as a chat contact, served
 * over local Tauri IPC (see `local-chat-transport.ts`) rather than the cloud
 * relay. Guarded by `isDesktop()` so a non-Tauri render (tests, dev web)
 * behaves like the cloud default.
 */

import { isDesktop } from "@/services/agent-runtime";
import { BUILTIN_RUNNER_ID } from "@/services/local-chat-transport";

export interface LocalChatContact {
  id: string;
  name: string;
  label?: string;
}

export function useLocalChatContacts(): LocalChatContact[] {
  if (!isDesktop()) return [];
  return [{ id: BUILTIN_RUNNER_ID, name: "Built-in agent", label: "Local" }];
}
