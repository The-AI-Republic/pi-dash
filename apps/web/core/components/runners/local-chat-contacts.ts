/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * Extra, non-cloud chat contacts for the runner picker (`RunnersSideNav`).
 *
 * This is a seam. In the shared cloud/self-hosted web app it returns nothing —
 * the only chat contacts are connected runners. The Tauri desktop build
 * overlays this file (`desktop-overlay/`) to add a locally-served built-in
 * agent entry, so that entry never appears in the cloud app.
 */

export interface LocalChatContact {
  /** Runner id used in the chat route (`.../chat/<id>`). */
  id: string;
  name: string;
  /** Short capability tag rendered beside the name, e.g. "Local". */
  label?: string;
}

/** No local contacts in the cloud web app. Overridden in the desktop build. */
export function useLocalChatContacts(): LocalChatContact[] {
  return [];
}
