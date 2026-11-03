// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Lazy boundary for the issues screen body. The route awaits this chunk in
// `beforeLoad` and then renders the resolved component directly, matching the
// list layout and empty-state leaves. React.lazy is deliberately not used:
// a boundary that suspends after the route commits discards the skeleton it
// just painted and mounts the screen again, which pushed desktop warm start
// past its budget (NEWFRONT-906, as NEWFRONT-619 for the list layout).

type IssuesListContentModule = typeof import("./IssuesListContent.js");
type IssuesListContentComponent = IssuesListContentModule["IssuesListContent"];

let request: Promise<IssuesListContentModule> | null = null;
let loaded: IssuesListContentComponent | null = null;

/** Start (or reuse) the screen chunk request. */
export function loadIssuesListContent(): Promise<IssuesListContentModule> {
  request ??= import("./IssuesListContent.js").then((module) => {
    loaded = module.IssuesListContent;
    return module;
  });
  return request;
}

/**
 * The screen component once `loadIssuesListContent()` has resolved. The route
 * calls this while rendering; a missing component means the await was skipped.
 */
export function resolvedIssuesListContent(): IssuesListContentComponent {
  if (loaded === null) {
    throw new Error("IssuesListContent was rendered before its chunk loaded; await loadIssuesListContent().");
  }
  return loaded;
}
