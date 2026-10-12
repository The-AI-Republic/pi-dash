/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { copyTextToClipboard } from "@pi-dash/utils";
import { desktopWebUrl } from "./desktop-web-url";

/**
 * Copy the shareable URL for an in-app path ("Copy link").
 *
 * Desktop replacement for the apps/web helper, which resolves the path
 * against the page origin — a Tauri-owned one here. Rejects, like a failed
 * clipboard write, when the bundle has no hosted origin to share.
 */
export const copyShareUrl = async (path: string): Promise<void> => {
  await copyTextToClipboard(desktopWebUrl(path));
};
