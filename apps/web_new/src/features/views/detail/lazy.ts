// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail chunk boundary (quality gates, §7). The route file is eager, so
// the screen — with the issues layout root, the five layout components and
// the create dialog it can reach — rides this request. The route starts it
// with its own module and awaits the same promise in `beforeLoad` beside
// the data reads, then renders the resolved component directly: a
// `React.lazy` boundary that suspends after the route commits would mount
// the screen, drop it for the fallback and mount it again.

type DetailModule = typeof import("./SavedViewScreen.js");
type PreloadModule = typeof import("./preload.js");

interface SavedViewDetailChunk {
  SavedViewScreen: DetailModule["SavedViewScreen"];
  preloadSavedView: PreloadModule["preloadSavedView"];
}

let request: Promise<SavedViewDetailChunk> | null = null;
let resolved: SavedViewDetailChunk | null = null;

/** Start (or reuse) the detail chunk request. */
export function loadSavedViewDetail(): Promise<SavedViewDetailChunk> {
  request ??= Promise.all([import("./SavedViewScreen.js"), import("./preload.js")]).then(([screen, preload]) => {
    resolved = { SavedViewScreen: screen.SavedViewScreen, preloadSavedView: preload.preloadSavedView };
    return resolved;
  });
  return request;
}

/** The screen once the chunk resolved; throws before the route's await. */
export function resolvedSavedViewDetail(): SavedViewDetailChunk {
  if (resolved === null) {
    throw new Error("SavedViewScreen rendered before its chunk loaded; await loadSavedViewDetail() in beforeLoad.");
  }
  return resolved;
}

type WorkspaceDetailModule = typeof import("./WorkspaceViewScreen.js");
type WorkspacePreloadModule = typeof import("./workspacePreload.js");

interface WorkspaceViewDetailChunk {
  WorkspaceViewScreen: WorkspaceDetailModule["WorkspaceViewScreen"];
  preloadWorkspaceView: WorkspacePreloadModule["preloadWorkspaceView"];
}

let workspaceRequest: Promise<WorkspaceViewDetailChunk> | null = null;
let workspaceResolved: WorkspaceViewDetailChunk | null = null;

/** Start (or reuse) the workspace detail chunk request. */
export function loadWorkspaceViewDetail(): Promise<WorkspaceViewDetailChunk> {
  workspaceRequest ??= Promise.all([import("./WorkspaceViewScreen.js"), import("./workspacePreload.js")]).then(
    ([screen, preload]) => {
      workspaceResolved = {
        WorkspaceViewScreen: screen.WorkspaceViewScreen,
        preloadWorkspaceView: preload.preloadWorkspaceView,
      };
      return workspaceResolved;
    }
  );
  return workspaceRequest;
}

/** The screen once the chunk resolved; throws before the route's await. */
export function resolvedWorkspaceViewDetail(): WorkspaceViewDetailChunk {
  if (workspaceResolved === null) {
    throw new Error(
      "WorkspaceViewScreen rendered before its chunk loaded; await loadWorkspaceViewDetail() in beforeLoad."
    );
  }
  return workspaceResolved;
}
