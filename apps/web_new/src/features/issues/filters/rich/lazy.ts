// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Lazy boundary for the issues rich filters (Quality gates). The route
// starts this chunk while its data loads, so the first layout render does
// not suspend on it: suspending after commit would mount the list, replace
// it with the fallback and mount it again (NEWFRONT-619).
import * as React from "react";

type FiltersComponent = (typeof import("./IssueFilters.js"))["default"];

let request: Promise<{ default: FiltersComponent }> | null = null;

/** Start (or reuse) the filter chunk request. */
export function loadIssueFilters(): Promise<{ default: FiltersComponent }> {
  request ??= import("./IssueFilters.js").then((module) => ({ default: module.default }));
  return request;
}

/** Kick the request while the route's data is still loading. */
export function preloadIssueFilters(): void {
  void loadIssueFilters().catch(() => {
    // The lazy boundary surfaces the failure when it renders.
  });
}

export const LazyIssueFilters = React.lazy(loadIssueFilters);
