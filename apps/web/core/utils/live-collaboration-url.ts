/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { liveServerLocation } from "./live-server-location";

/**
 * Build the live server's collaboration WebSocket URL.
 *
 * ws vs wss follows the live server's own scheme, not the page's: a desktop
 * page runs on tauri:// or http://tauri.localhost whatever the server uses.
 */
export const liveCollaborationUrl = (params: Record<string, unknown> = {}): URL => {
  const { baseUrl, basePath } = liveServerLocation();
  const url = new URL(baseUrl);
  const isSecure = url.protocol === "https:" || url.protocol === "wss:";
  url.protocol = isSecure ? "wss" : "ws";
  url.pathname = `${basePath}/collaboration`;

  Object.entries(params)
    .filter(([_, value]) => value !== undefined && value !== null)
    .forEach(([key, value]) => {
      url.searchParams.set(key, String(value));
    });

  return url;
};
