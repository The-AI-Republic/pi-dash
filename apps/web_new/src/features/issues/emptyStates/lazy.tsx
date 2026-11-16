// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Lazy boundary for the project list's empty cards (parity/specs/issues.md
// §8, ISS-068). The cards, their artwork and the expression parser are
// larger than the boolean that picks between them, and the route's first
// paint is a list of rows, so the route awaits this chunk with its other
// leaves and then renders the resolved component (Quality gates, as with
// the list leaves in ../components/lazyLeaves.tsx). Nothing here may
// import the card module statically: that would pull it back into the
// route's chunk.

import * as React from "react";

import { isFilteredEmpty } from "./filteredEmpty.js";

type CardModule = typeof import("./cards.js");

let request: Promise<CardModule> | null = null;
let resolved: CardModule | null = null;

function load(): Promise<CardModule> {
  request ??= import("./cards.js").then((module) => {
    resolved = module;
    return module;
  });
  return request;
}

/** Await the chunk; the route calls this in `beforeLoad`. */
export function loadProjectEmptyState(): Promise<void> {
  return load().then(() => undefined);
}

export interface ProjectIssuesEmptyStateProps {
  /** Conditions the filter row holds right now. */
  hasCompleteCondition: boolean;
  /** A condition was added through the filter row in this visit. */
  addedThisVisit: boolean;
  /** Rows the current expression returned. */
  visibleCount: number;
  canAct: boolean;
  onClear: () => void;
  onCreate: () => void;
}

/**
 * The card a project's issue list shows while it holds no rows: the
 * first-work-item prompt until a condition was added through the filter
 * row during this visit, the no-results card with clear-all from then on.
 * Both slots of the layout root render this one card.
 */
export function ProjectIssuesEmptyState({
  hasCompleteCondition,
  addedThisVisit,
  visibleCount,
  canAct,
  onClear,
  onCreate,
}: ProjectIssuesEmptyStateProps): React.ReactElement {
  const cards = resolved;
  if (cards === null) {
    throw new Error("ProjectIssuesEmptyState rendered before its chunk loaded; await loadProjectEmptyState().");
  }
  if (isFilteredEmpty({ mode: "visit", hasCompleteCondition, addedThisVisit, visibleCount })) {
    return (
      <cards.FilteredEmptyCard context="project" clearLabel="Clear all filters" canAct={canAct} onClear={onClear} />
    );
  }
  return <cards.ProjectIssuesEmpty canAct={canAct} onCreate={onCreate} />;
}
