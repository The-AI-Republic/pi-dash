/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * Desktop override of the session-teardown seam: clear the webview's own
 * cookie jar (plus local/session storage and caches) when signing out.
 *
 * The server's cookie deletions never reach this webview — the page origin is
 * `tauri://localhost` and the API is a different origin, so `Set-Cookie` on
 * that cross-origin response is dropped. Without this the session survives a
 * sign-out: the app lands on the sign-in page, the route guard sees a live
 * session, and sends the user straight back in.
 *
 * Runs after the sign-out request (which needs the CSRF cookie) and before the
 * navigation. Failure is swallowed: a sign-out that cannot clear local data
 * must still land the user on the sign-in page rather than throw.
 */

import { isDesktop } from "@/services/agent-runtime";

type Native = { core: { invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> } };

export async function clearDesktopSessionData(): Promise<void> {
  if (!isDesktop()) return;
  try {
    await (window as unknown as { __TAURI__: Native }).__TAURI__.core.invoke<void>("desktop_clear_web_data");
  } catch {
    // Nothing useful to do here — the navigation below is still the right
    // outcome, and the next sign-in overwrites whatever survived.
  }
}
