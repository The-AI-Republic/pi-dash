// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Collaboration sync loader (parity/specs/pages.md §8.4, PAGE-067). The
// centered mark shows only while the local cache is known-empty and the
// document is unsynced; warm caches and synced documents paint the
// editor straight away.

import * as React from "react";

import { PageDetailSpinner } from "./PageDetailLoading.js";

/** The wait surface for an empty cache with no synced document yet. */
export function PageLiveLoader(): React.JSX.Element {
  return (
    <div
      data-testid="page-live-loader"
      className="grid size-full min-h-64 place-items-center"
      role="status"
      aria-label="Syncing page content"
    >
      <PageDetailSpinner />
    </div>
  );
}
