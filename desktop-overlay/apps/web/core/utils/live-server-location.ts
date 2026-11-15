/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { API_BASE_URL, LIVE_BASE_PATH, LIVE_BASE_URL } from "@pi-dash/constants";

export type TLiveServerLocation = {
  /** http(s) origin the live server is reached on. */
  baseUrl: string;
  /** Path prefix the live server is mounted under ("" or e.g. "/live"). */
  basePath: string;
};

/**
 * Where the live (real-time collaboration) server is.
 *
 * Desktop replacement for the apps/web helper, which falls back to the page
 * origin — a Tauri-owned one here (tauri://localhost, or
 * http://tauri.localhost on Windows) that serves no live server. Desktop
 * builds always bake the server's VITE_API_BASE_URL, and a self-hosted server
 * mounts the live server at /live on that same host (apps/proxy/Caddyfile.ce),
 * so that is the default; VITE_LIVE_BASE_URL / VITE_LIVE_BASE_PATH override it.
 */
export const liveServerLocation = (): TLiveServerLocation => {
  const baseUrl = LIVE_BASE_URL.trim() || API_BASE_URL.trim();
  if (!baseUrl) throw new Error("VITE_API_BASE_URL is required to reach the live server from the desktop app");

  const { protocol } = new URL(baseUrl);
  if (protocol !== "http:" && protocol !== "https:") {
    throw new Error("The desktop live server origin must use http or https");
  }

  return { baseUrl, basePath: LIVE_BASE_PATH.trim() || "/live" };
};
