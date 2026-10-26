// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pulls the page-export registration chunk on demand (PAGE-074, NEWFRONT-846).
// The route starts this load from its mount effect, so a worker closing while
// the chunk is still in flight must not surface as an unhandled rejection. A
// failed load simply leaves the dialog unregistered: the Pages detail then has
// no export entry, which is the same state as before the chunk arrived.

/** Starts the registration chunk; resolves false when that chunk cannot load. */
export async function loadPageExportRegistration(load: () => Promise<unknown>): Promise<boolean> {
  try {
    await load();
    return true;
  } catch {
    return false;
  }
}
