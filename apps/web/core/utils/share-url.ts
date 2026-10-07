/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { copyUrlToClipboard } from "@pi-dash/utils";

/**
 * Copy the shareable URL for an in-app path ("Copy link").
 *
 * On the web the page origin is the shareable origin. The desktop app is not
 * served from one, so desktop-overlay/ replaces this file; call this instead
 * of copyUrlToClipboard() so a copied link works outside the desktop app too.
 */
export const copyShareUrl = (path: string): Promise<void> => copyUrlToClipboard(path);
