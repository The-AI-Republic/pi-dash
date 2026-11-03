// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Lazy boundary for the board layout (Quality gates). The issues route
// awaits this chunk beside its data and then renders the resolved
// component directly, the same handshake the list layout uses: a
// Suspense boundary that suspends after the route commits would mount the
// board twice and push the desktop warm start past its budget.
type KanbanLayoutModule = typeof import("./KanbanBoard.js");
type KanbanLayoutComponent = KanbanLayoutModule["KanbanBoard"];

let request: Promise<{ default: KanbanLayoutComponent }> | null = null;
let loaded: KanbanLayoutComponent | null = null;

/** Start (or reuse) the board chunk request. */
export function loadKanbanLayout(): Promise<{ default: KanbanLayoutComponent }> {
  request ??= import("./KanbanBoard.js").then((module) => {
    loaded = module.KanbanBoard;
    return { default: module.KanbanBoard };
  });
  return request;
}

/** The board component once its chunk resolved; throws before the await. */
export function resolvedKanbanLayout(): KanbanLayoutComponent {
  if (loaded === null) {
    throw new Error("KanbanBoard was rendered before its chunk loaded; await loadKanbanLayout() in beforeLoad.");
  }
  return loaded;
}
