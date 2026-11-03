// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Lazy door to the built-in settings bodies (parity/specs/profile.md §5,
// PROF-009/010; main#670). Account settings now live inside the workspace
// tree, so their layout's beforeLoad ask for the bodies: this module only
// remembers one request, and the registry side of the seam arrives in its
// own chunk instead of joining the shell and issue-list first paint. A
// second account visit reuses the settled promise, so navigating between
// tabs never re-registers (and never flickers back to the placeholder).

let loaded: Promise<void> | null = null;

/** Registers the built-in tab bodies once; resolves when they are present. */
export function loadAccountSettingsTabs(): Promise<void> {
  loaded ??= import("./pageEntries.js").then(
    () => undefined,
    (error: unknown) => {
      // A failed fetch must not poison every later visit: forget the request
      // so the next navigation tries again.
      loaded = null;
      throw error;
    }
  );
  return loaded;
}

/** Drops the memoized request. Tests only. */
export function resetAccountSettingsTabsLoader(): void {
  loaded = null;
}
