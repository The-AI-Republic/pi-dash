// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The leaf modules the issue list screens mount in their body: the
// virtualized list and the header analytics entry. Both are heavy relative
// to the shell (the list carries the row virtualizer, the analytics entry
// carries the dialog), so the screens load them as their own chunk. That
// keeps the initial chain inside its budget (Quality gates) while the
// first paint still renders the real list.
//
// The routes render the resolved component directly instead of going
// through React.lazy. A Suspense boundary that suspends after the route
// commits makes the list mount, fall back to the skeleton and mount again;
// on a warm start that re-mount pushed the desktop startup past its budget
// (NEWFRONT-669, the same pitfall NEWFRONT-619 recorded for the list
// layout). The route starts the chunk with its own module and awaits the
// same request in `beforeLoad`, so the warm start does not serialize a
// round trip behind the data reads.

import * as React from "react";

type IssueListModule = typeof import("./IssueList.js");
type AnalyticsEntryModule = typeof import("../analytics/AnalyticsEntry.js");

interface IssueListLeaves {
  IssueList: IssueListModule["IssueList"];
  AnalyticsEntry: AnalyticsEntryModule["AnalyticsEntry"];
}

let request: Promise<IssueListLeaves> | null = null;
let resolved: IssueListLeaves | null = null;

function start(): Promise<IssueListLeaves> {
  request ??= Promise.all([import("./IssueList.js"), import("../analytics/AnalyticsEntry.js")]).then(
    ([list, analytics]) => {
      resolved = { IssueList: list.IssueList, AnalyticsEntry: analytics.AnalyticsEntry };
      return resolved;
    }
  );
  return request;
}

/**
 * Start the chunk without awaiting it, so it travels beside the route's
 * first data read. A failure is left for `loadIssueListLeaves()` to report:
 * the route turns it into its load error instead of a stray rejection.
 */
export function primeIssueListLeaves(): void {
  start().catch(() => {});
}

/** Await the chunk; the route calls this in `beforeLoad`, after its reads. */
export function loadIssueListLeaves(): Promise<void> {
  return start().then(() => undefined);
}

/** The leaf components, once the chunk has resolved. */
export function resolvedIssueListLeaves(): IssueListLeaves {
  if (resolved === null) {
    throw new Error("Issue list leaves rendered before their chunk loaded; await loadIssueListLeaves() in beforeLoad.");
  }
  return resolved;
}

/** The rendered list leaf, for routes whose `beforeLoad` awaited the chunk. */
export function ResolvedIssueList(props: React.ComponentProps<IssueListModule["IssueList"]>): React.ReactElement {
  const Component = resolvedIssueListLeaves().IssueList;
  return <Component {...props} />;
}

/** The rendered analytics entry, for routes whose `beforeLoad` awaited the chunk. */
export function ResolvedAnalyticsEntry(
  props: React.ComponentProps<AnalyticsEntryModule["AnalyticsEntry"]>
): React.ReactElement | null {
  const Component = resolvedIssueListLeaves().AnalyticsEntry;
  return <Component {...props} />;
}
