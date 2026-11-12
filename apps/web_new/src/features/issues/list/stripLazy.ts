// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Lazy boundary for the row property strip (Quality gates). The strip
// brings the property pickers; its chunk stays out of the list's first
// route chain so rows paint before the picker code arrives (desktop
// warm-start budget). The route primes this request with its own module so
// the chunk is usually resolved by first render and rows mount full; when
// it is not, rows paint as shells and mount the strip on idle
// (NEWFRONT-911). Suspending the rows on the chunk instead delayed first
// paint.
export type InlineStripComponent = (typeof import("./InlinePropertyStrip.js"))["InlinePropertyStrip"];

type StripComponent = InlineStripComponent;

let request: Promise<{ default: StripComponent }> | null = null;
let loaded: StripComponent | null = null;

/** Start (or reuse) the strip chunk request. */
export function loadInlineStrip(): Promise<{ default: StripComponent }> {
  request ??= import("./InlinePropertyStrip.js").then((module) => {
    loaded = module.InlinePropertyStrip;
    return { default: module.InlinePropertyStrip };
  });
  return request;
}

/** Kick the request with the route module so rows mount with their strip. */
export function primeInlineStrip(): void {
  void loadInlineStrip().catch(() => {
    // The rows retry the same request when they mount the strip.
  });
}

/** The strip component once `loadInlineStrip()` has resolved, else null. */
export function loadedInlineStrip(): StripComponent | null {
  return loaded;
}
