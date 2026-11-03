// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Lazy entry to the saved-view empty card (parity/specs/views.md §7,
// VIEW-028). The card carries the view artwork, so it stays out of the
// eager issues barrel: this module holds only the chunk request and a thin
// component, and the views detail preload awaits the chunk beside its data
// read. Rendering before that await is a routing mistake, not a user state.

import * as React from "react";

type CardModule = typeof import("./cards.js");

let request: Promise<CardModule> | null = null;
let resolved: CardModule | null = null;

/** Start (or reuse) the card chunk. */
function load(): Promise<CardModule> {
  request ??= import("./cards.js").then((module) => {
    resolved = module;
    return module;
  });
  return request;
}

/** Await the card chunk; the view detail preload calls this. */
export function loadSavedViewEmptyCard(): Promise<void> {
  return load().then(() => undefined);
}

export interface SavedViewEmptyCardProps {
  /** Project guests see the create entry disabled. */
  canAct: boolean;
  onCreate: () => void;
}

/** The card a saved view shows when its stored conditions match nothing. */
export function SavedViewEmptyCard({ canAct, onCreate }: SavedViewEmptyCardProps): React.ReactElement {
  const cards = resolved;
  if (cards === null) {
    throw new Error("SavedViewEmptyCard rendered before its chunk loaded; await loadSavedViewEmptyCard().");
  }
  return <cards.SavedViewIssuesEmpty canAct={canAct} onCreate={onCreate} />;
}
