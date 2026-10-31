// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake chunk boundary (Quality gates, initial JS; NEWFRONT-869). The
// route and the project layout that hosts it sit in the first paint's
// closure, so the screen, the create dialog, their reads and the api-client
// contract behind them ride this request. The route starts it with its own
// module and awaits it in `beforeLoad` beside the data reads, then renders
// the resolved components directly: a `React.lazy` boundary that suspends
// after the route commits would mount the screen, drop it for the fallback
// and mount it again.

type ScreenModule = typeof import("../components/IntakeScreen.js");
type DialogModule = typeof import("../components/IntakeCreateDialog.js");
type PreloadModule = typeof import("./preload.js");

interface IntakeRouteChunk {
  IntakeScreen: ScreenModule["IntakeScreen"];
  IntakeCreateDialog: DialogModule["IntakeCreateDialog"];
  preloadIntakeRoute: PreloadModule["preloadIntakeRoute"];
}

let request: Promise<IntakeRouteChunk> | null = null;
let resolved: IntakeRouteChunk | null = null;

/** Start (or reuse) the route chunk request. */
export function loadIntakeRouteChunk(): Promise<IntakeRouteChunk> {
  request ??= Promise.all([
    import("../components/IntakeScreen.js"),
    import("../components/IntakeCreateDialog.js"),
    import("./preload.js"),
  ]).then(([screen, dialog, preload]) => {
    resolved = {
      IntakeScreen: screen.IntakeScreen,
      IntakeCreateDialog: dialog.IntakeCreateDialog,
      preloadIntakeRoute: preload.preloadIntakeRoute,
    };
    return resolved;
  });
  return request;
}

/** The chunk once it resolved; throws before the route's await. */
export function resolvedIntakeRouteChunk(): IntakeRouteChunk {
  if (resolved === null) {
    throw new Error("IntakeScreen rendered before its chunk loaded; await loadIntakeRouteChunk() in beforeLoad.");
  }
  return resolved;
}
