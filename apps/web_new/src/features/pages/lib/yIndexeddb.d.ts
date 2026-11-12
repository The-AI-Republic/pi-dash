// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Local document cache (parity/specs/pages.md §8.4, PAGE-067). The
// persistence library ships no types; this is the narrow slice the
// collaboration backend uses: construct per page, wait for the cached
// updates to land, tear down with the session.

declare module "y-indexeddb" {
  export class IndexeddbPersistence {
    constructor(name: string, doc: unknown);
    readonly synced: boolean;
    on(event: "synced", listener: () => void): void;
    off(event: "synced", listener: () => void): void;
    destroy(): void;
  }
}
