// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake chunk boundary (Quality gates, initial JS; NEWFRONT-869). The
// route and the project layout that hosts it sit in the first paint's
// closure, so the screen, its reads and the api-client contract behind
// them ride this request. The route starts it with its own module and
// awaits it in `beforeLoad` beside the data reads, then renders the
// resolved component directly: a `React.lazy` boundary that suspends after
// the route commits would mount the screen, drop it for the fallback and
// mount it again.

type ScreenModule = typeof import("../components/IntakeScreen.js");
type PreloadModule = typeof import("./preload.js");

interface IntakeRouteChunk {
  IntakeScreen: ScreenModule["IntakeScreen"];
  preloadIntakeRoute: PreloadModule["preloadIntakeRoute"];
}

let request: Promise<IntakeRouteChunk> | null = null;
let resolved: IntakeRouteChunk | null = null;

/** Start (or reuse) the route chunk request. */
export function loadIntakeRouteChunk(): Promise<IntakeRouteChunk> {
  request ??= Promise.all([import("../components/IntakeScreen.js"), import("./preload.js")]).then(
    ([screen, preload]) => {
      resolved = { IntakeScreen: screen.IntakeScreen, preloadIntakeRoute: preload.preloadIntakeRoute };
      return resolved;
    }
  );
  return request;
}

/** The screen once the chunk resolved; throws before the route's await. */
export function resolvedIntakeRouteChunk(): IntakeRouteChunk {
  if (resolved === null) {
    throw new Error("IntakeScreen rendered before its chunk loaded; await loadIntakeRouteChunk() in beforeLoad.");
  }
  return resolved;
}
