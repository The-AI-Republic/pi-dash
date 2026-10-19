// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The leaf modules the issue list screens mount in their body: the
// virtualized list and the header analytics entry. Both are heavy relative
// to the shell (the list carries the row virtualizer, the analytics entry
// carries the dialog), so the screens load them through React.lazy and
// preload them in `beforeLoad`, together with the data they read. That
// keeps the first paint inside the initial-JS budget (Quality gates)
// without a skeleton flash: the chunk is already resolved when the screen
// commits.

import * as React from "react";

import { ListSkeleton } from "../loaders/ListSkeleton.js";

export const LazyIssueList = React.lazy(() =>
  import("./IssueList.js").then((module) => ({ default: module.IssueList }))
);

export const LazyAnalyticsEntry = React.lazy(() =>
  import("../analytics/AnalyticsEntry.js").then((module) => ({ default: module.AnalyticsEntry }))
);

/**
 * Resolve both leaf chunks. `beforeLoad` awaits this after its data reads,
 * so the route's first commit renders the real list instead of the
 * skeleton. Failures are swallowed: `React.lazy` retries on mount, and the
 * route's error boundary owns a genuine chunk failure.
 */
export async function preloadIssueListLeaves(): Promise<void> {
  try {
    await Promise.all([import("./IssueList.js"), import("../analytics/AnalyticsEntry.js")]);
  } catch {
    // The lazy boundaries surface the error when the screen mounts.
  }
}

/** Suspense fallback for the list leaf; also the screen's pending window. */
export function IssueListFallback(): React.ReactElement {
  return <ListSkeleton />;
}
