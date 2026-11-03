// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Lazy boundary for the list layout (Quality gates). The route awaits the
// chunk in `beforeLoad` and then renders the resolved component directly.
// React.lazy is deliberately not used: a Suspense boundary that suspends
// after the route commits mounts the layout, discards it for the fallback
// and mounts it again, which pushed the desktop warm start past its budget
// (NEWFRONT-619). The dynamic import keeps the chunk out of the initial
// chain that the size gate measures.
type ListLayoutModule = typeof import("./ListLayout.js");
type ListLayoutComponent = ListLayoutModule["ListLayout"];

let request: Promise<{ default: ListLayoutComponent }> | null = null;
let loaded: ListLayoutComponent | null = null;

/** Start (or reuse) the layout chunk request. */
export function loadListLayout(): Promise<{ default: ListLayoutComponent }> {
  request ??= import("./ListLayout.js").then((module) => {
    loaded = module.ListLayout;
    return { default: module.ListLayout };
  });
  return request;
}

/**
 * The layout component once `loadListLayout()` has resolved. Routes that
 * await the chunk in `beforeLoad` call this while rendering; a missing
 * component means the route skipped the await.
 */
export function resolvedListLayout(): ListLayoutComponent {
  if (loaded === null) {
    throw new Error("ListLayout was rendered before its chunk loaded; await loadListLayout() in beforeLoad.");
  }
  return loaded;
}
