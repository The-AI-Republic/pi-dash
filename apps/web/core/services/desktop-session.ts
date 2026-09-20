/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * Desktop session teardown seam.
 *
 * In a browser, signing out is finished by the server's `Set-Cookie`
 * deletions. In the Tauri desktop build those deletions are dropped — the page
 * origin is `tauri://localhost` and the API is a different origin — so the
 * session outlives the sign-out and the app bounces back in. The desktop
 * overlay replaces this file with one that clears the webview's own cookie jar.
 *
 * No-op everywhere else, so sign-out reads the same in both builds.
 */
export async function clearDesktopSessionData(): Promise<void> {}
