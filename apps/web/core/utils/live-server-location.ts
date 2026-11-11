/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { LIVE_BASE_PATH, LIVE_BASE_URL } from "@pi-dash/constants";

export type TLiveServerLocation = {
  /** http(s) origin the live server is reached on. */
  baseUrl: string;
  /** Path prefix the live server is mounted under ("" or e.g. "/live"). */
  basePath: string;
};

/**
 * Where the live (real-time collaboration) server is.
 *
 * On the web it is served from the page origin unless VITE_LIVE_BASE_URL
 * names another host. The desktop app is not served from one, so
 * desktop-overlay/ replaces this file.
 */
export const liveServerLocation = (): TLiveServerLocation => ({
  baseUrl: LIVE_BASE_URL.trim() || window.location.origin,
  basePath: LIVE_BASE_PATH,
});
