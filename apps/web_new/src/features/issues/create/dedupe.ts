// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Duplicate detection seam for the create dialog (parity/specs/issues.md
// §11 ISS-121 and §23). The dialog always asks this hook; the community
// edition answers none, so its count button and side-by-side panel never
// mount. A cloud build replaces the hook behind the edition flag and
// supplies the debounced search; the dialog consumes only this shape.

import type { IssueSearchHit } from "@pidash/api-client";
import * as React from "react";

/** Edition flag a cloud build sets to turn duplicate matching on. */
export const ISSUE_DEDUPE_FLAG = "issue-dedupe";
/** Idle pause before a title keystroke burst searches for duplicates. */
export const DEDUPE_DEBOUNCE_MS = 500;

/** What the dialog renders from the seam: at most a count and the rows. */
export interface DuplicateMatches {
  matches: IssueSearchHit[];
  isLoading: boolean;
  /** True when the surface should show its count button and panel. */
  isEnabled: boolean;
}

const NO_MATCHES: DuplicateMatches = { matches: [], isLoading: false, isEnabled: false };

export interface DuplicateSearchInput {
  workspaceSlug: string;
  projectId: string;
  title: string;
  enabled: boolean;
}

/**
 * Matching work items for a typed title. This build has no server-side
 * duplicate read, so it never matches and never enables the surface.
 */
export function useDuplicateMatches(_input: DuplicateSearchInput): DuplicateMatches {
  return NO_MATCHES;
}

/**
 * The side-by-side panel the cloud seam feeds (ISS-121). An empty result
 * renders nothing, so the community build shows no trace of the feature.
 */
export function DuplicatePanel({ matches }: { matches: readonly IssueSearchHit[] }): React.ReactElement | null {
  if (matches.length === 0) return null;
  return React.createElement(
    "aside",
    { "aria-label": "Possible duplicates", className: "flex flex-col gap-(--space-1)" },
    React.createElement(
      "p",
      { className: "text-meta text-(--text-muted)" },
      `${matches.length} possible duplicate${matches.length === 1 ? "" : "s"}`
    ),
    React.createElement(
      "ul",
      { className: "flex flex-col gap-(--space-1)" },
      matches.map((match) =>
        React.createElement("li", { key: match.id, className: "text-body text-(--text)" }, match.name)
      )
    )
  );
}

/** Whether the title is long enough to spend a search on it. */
export function shouldSearchDuplicates(title: string, enabled: boolean): boolean {
  return enabled && title.trim().length >= 3;
}
