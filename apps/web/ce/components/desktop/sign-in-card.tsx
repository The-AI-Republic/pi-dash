/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * Edition seam: the sign-in card the desktop app shows to signed-out users
 * (desktop-overlay `app/(home)/page.tsx`). Editions whose server supports
 * the desktop sign-in hand-off (`pidash://auth/callback` →
 * `/api/auth/desktop-exchange/`) replace this file wholesale — keep the
 * exported `DesktopSignInCard` name stable.
 *
 * The community server has no desktop sign-in hand-off yet, so this card
 * points the user at the web app instead of offering a form that could
 * not establish a desktop session. It deliberately does not say "Sign in to
 * Pi Dash": that heading is what an edition's sign-in card is recognised by
 * (build.rs with PIDASH_DESKTOP_EXTERNAL_SIGNIN=1), so a build that expects
 * an edition card but ships this one fails instead of passing.
 */

import { WEB_URL } from "@pi-dash/constants";
import { Button } from "@pi-dash/propel/button";

type TauriCore = { invoke(command: string, args?: Record<string, unknown>): Promise<unknown> };

function openWebApp() {
  const core = (window as unknown as { __TAURI__?: { core?: TauriCore } }).__TAURI__?.core;
  if (!core) {
    window.location.assign(WEB_URL);
    return;
  }
  core.invoke("open_in_browser", { url: WEB_URL }).catch((error: unknown) => {
    console.error("open_in_browser failed", error);
  });
}

export function DesktopSignInCard() {
  return (
    <div className="shadow-md flex w-full max-w-md flex-col items-center gap-6 rounded-2xl border border-subtle bg-surface-1 p-8 text-center sm:p-10">
      <div className="flex flex-col items-center gap-2">
        <h2 className="text-h2-semibold text-primary">Desktop sign-in isn&apos;t available yet</h2>
        <p className="text-body-md-regular text-secondary">
          This server doesn&apos;t support signing in from the desktop app yet. You can keep using Pi Dash in your
          browser in the meantime.
        </p>
      </div>
      {WEB_URL ? (
        <Button variant="primary" size="xl" className="w-full" onClick={openWebApp}>
          Open Pi Dash in your browser
        </Button>
      ) : null}
    </div>
  );
}
