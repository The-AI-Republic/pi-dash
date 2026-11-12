// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Reload seam for the custom designer (parity/specs/profile.md §7,
// PROF-016). Saving a palette persists first and then re-enters the app so
// every surface boots from the saved colors. Hosts without a reload hook
// (unit tests) simply keep the applied theme.

/** Reload the running document; no-op where no window is available. */
export function reloadDocument(): void {
  if (typeof window === "undefined") return;
  window.location.reload();
}
