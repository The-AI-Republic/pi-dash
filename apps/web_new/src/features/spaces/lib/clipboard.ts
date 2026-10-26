// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Clipboard-write gate for the public overlay's copy action (SPC-045).
// The action exists only where the browser can actually write: the
// clipboard API must be present and, when the permissions API answers
// for `clipboard-write`, it must not say denied. A browser that does not
// answer the query at all keeps the action (absence of a refusal is not
// a refusal) rather than hiding it from every visitor.

import * as React from "react";

/** The clipboard surface the gate reads; absent in insecure contexts. */
export interface ClipboardCapable {
  clipboard?: { writeText?: ((text: string) => Promise<void>) | undefined } | undefined;
  permissions?: { query?: ((descriptor: { name: "clipboard-write" }) => Promise<{ state: string }>) | undefined };
}

/** Whether the browser exposes clipboard writing at all. */
export function clipboardWriteAvailable(navigatorLike: Pick<ClipboardCapable, "clipboard"> | undefined): boolean {
  return typeof navigatorLike?.clipboard?.writeText === "function";
}

/** Whether the permission probe says the write is refused. */
export function clipboardPermissionRefused(permission: { state: string }): boolean {
  return permission.state === "denied";
}

/**
 * Whether the overlay may offer the copy action. Resolves false on a
 * browser without clipboard writing, and false once the permission
 * probe answers denied; a probe failure is not a refusal.
 */
export async function readClipboardWriteAllowed(navigatorLike: ClipboardCapable | undefined): Promise<boolean> {
  if (!clipboardWriteAvailable(navigatorLike)) return false;
  const query = navigatorLike?.permissions?.query;
  if (typeof query !== "function") return true;
  try {
    const status = await query.call(navigatorLike?.permissions, { name: "clipboard-write" });
    return !clipboardPermissionRefused(status);
  } catch {
    return true;
  }
}

/** Starts closed (the action never flashes) and settles after the probe. */
export function useClipboardWritePermission(): boolean {
  const [allowed, setAllowed] = React.useState(false);
  React.useEffect(() => {
    let live = true;
    // The DOM lib types the permission descriptor name as the whole
    // `PermissionName` union; our probe only needs the one member.
    const navigatorLike = globalThis.navigator as unknown as ClipboardCapable | undefined;
    void readClipboardWriteAllowed(navigatorLike).then((next) => {
      if (live) setAllowed(next);
    });
    return () => {
      live = false;
    };
  }, []);
  return allowed;
}
