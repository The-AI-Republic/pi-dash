// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Lazy boundary for the issues rich filters (Quality gates). The route
// starts this chunk while its data loads, so the first layout render does
// not suspend on it: suspending after commit would mount the list, replace
// it with the fallback and mount it again (NEWFRONT-619).
//
// Routes that await the chunk in `beforeLoad` render the resolved component
// directly instead of going through React.lazy: the lazy payload reads as
// pending on first render even when the chunk resolved long before, so the
// boundary suspends, commits the fallback, and retries far too late — on
// the issues route that retry landed inside the 2,000-row scroll window and
// pushed frames past the budget (NEWFRONT-911, as NEWFRONT-906 for the
// screen). Screens that cannot await keep using `LazyIssueFilters`.
import * as React from "react";

type FiltersComponent = (typeof import("./IssueFilters.js"))["default"];

let request: Promise<{ default: FiltersComponent }> | null = null;
let loaded: FiltersComponent | null = null;

/** Start (or reuse) the filter chunk request. */
export function loadIssueFilters(): Promise<{ default: FiltersComponent }> {
  request ??= import("./IssueFilters.js").then((module) => {
    loaded = module.default;
    return { default: module.default };
  });
  return request;
}

/** Kick the request while the route's data is still loading. */
export function preloadIssueFilters(): void {
  void loadIssueFilters().catch(() => {
    // The lazy boundary surfaces the failure when it renders.
  });
}

/**
 * The filters component once `loadIssueFilters()` has resolved. Routes that
 * await the chunk in `beforeLoad` call this while rendering; a missing
 * component means the route skipped the await.
 */
export function resolvedIssueFilters(): FiltersComponent {
  if (loaded === null) {
    throw new Error("IssueFilters was rendered before its chunk loaded; await loadIssueFilters() in beforeLoad.");
  }
  return loaded;
}

export const LazyIssueFilters = React.lazy(loadIssueFilters);
