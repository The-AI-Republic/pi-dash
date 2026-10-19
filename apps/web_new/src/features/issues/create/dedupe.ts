// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Duplicate detection seam for the create dialog (parity/specs/issues.md
// §11 ISS-121 and §23). The dialog always asks this hook; the community
// edition answers none, so its count button and side-by-side panel never
// mount. A cloud build replaces the hook behind the edition flag and
// supplies the debounced search; the dialog consumes only this shape.

import { useQuery } from "@tanstack/react-query";
import { searchIssues, type IssueSearchHit, type IssueSearchResults } from "@pidash/api-client";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { ISSUE_DEDUPE_FLAG } from "../../../core/edition/index.js";
import { REFERENCE_STALE_TIME_MS, workspaceScope } from "../../../core/query/client.js";
import { useDebouncedValue } from "../../../shared/commands/index.js";

/** Edition flag a cloud build sets to turn duplicate matching on. */
export { ISSUE_DEDUPE_FLAG };
/** Idle pause before a title keystroke burst searches for duplicates. */
export const DEDUPE_DEBOUNCE_MS = 500;

/** What the dialog renders from the seam: at most a count and the rows. */
export interface DuplicateMatches {
  matches: IssueSearchHit[];
  isLoading: boolean;
  /** True when the surface should show its count button and panel. */
  isEnabled: boolean;
}

export const NO_MATCHES: DuplicateMatches = { matches: [], isLoading: false, isEnabled: false };

export interface DuplicateSearchInput {
  workspaceSlug: string;
  projectId: string;
  title: string;
  enabled: boolean;
}

/** Cache key prefix for the dialog's duplicate search. */
export function duplicateSearchKey(workspaceSlug: string, projectId: string, title: string) {
  return [...workspaceScope(workspaceSlug), "issues", "create", "duplicates", projectId, title] as const;
}

/**
 * The edition's duplicate read: a search for the typed title scoped to
 * the item's project. The old app's similar-issue panel searched the
 * same endpoint the parent picker uses, so a passing title is a query,
 * not a special server call.
 */
export function useDuplicateSearch(workspaceSlug: string, projectId: string, title: string, enabled: boolean) {
  return useQuery({
    queryKey: duplicateSearchKey(workspaceSlug, projectId, title),
    queryFn: (): Promise<IssueSearchResults> =>
      searchIssues(getAppClient(), workspaceSlug, projectId, { search: title }),
    staleTime: REFERENCE_STALE_TIME_MS,
    enabled: enabled && projectId !== "",
  });
}

/**
 * Title words worth comparing. Short grammar words carry no meaning and
 * would otherwise make every item sharing "the" a duplicate.
 */
const STOP_WORDS = new Set(["a", "an", "and", "for", "in", "of", "on", "the", "to", "with"]);

/** The compared words of a title: lower-case, punctuation folded away. */
export function titleWords(title: string): string[] {
  return title
    .toLowerCase()
    .split(/[^\p{L}\p{N}]+/u)
    .filter((word) => word !== "" && !STOP_WORDS.has(word));
}

/**
 * Whether two titles name the same work, typed differently: identical
 * once case, punctuation and spacing are folded; one is a single word
 * the other also uses ("Login" / "Login page crashes"); or they share
 * more than half their meaningful words ("Fix login redirect" /
 * "Login redirect loop"). Sharing one word of two is not enough.
 */
export function isNearDuplicateTitle(candidate: string, title: string): boolean {
  const left = titleWords(candidate);
  const right = titleWords(title);
  if (left.length === 0 || right.length === 0) return false;
  if (left.join(" ") === right.join(" ")) return true;
  if (left.length === 1 || right.length === 1) {
    const [shorter, longer] = left.length === 1 ? [left, right] : [right, left];
    const word = shorter[0]!;
    return word.length >= 3 && longer.includes(word);
  }
  const shared = left.filter((word) => right.includes(word)).length;
  // The shorter title must agree on more than half its words, so a
  // single shared word of two is not enough.
  return shared * 2 > Math.min(left.length, right.length);
}

/**
 * Near-duplicate rows in the project the form targets. The search is
 * full-text, so it can also match a body word; the title comparison is
 * what decides, and rows from another project never count (ISS-121).
 */
export function selectDuplicateMatches(
  hits: readonly IssueSearchHit[],
  title: string,
  projectId: string
): IssueSearchHit[] {
  if (title.trim() === "") return [];
  return hits.filter((hit) => hit.project_id === projectId && isNearDuplicateTitle(hit.name, title));
}

/**
 * Matching work items for a typed title. With the flag off (every OSS
 * build) the hook short-circuits: no read, no matches, surface off. With
 * it on, the title debounces, the query runs, and the count button and
 * panel mount from the result (ISS-121, §23).
 */
export function useDuplicateMatches(input: DuplicateSearchInput): DuplicateMatches {
  const active = input.enabled && shouldSearchDuplicates(input.title, input.enabled);
  const term = useDebouncedValue(active ? input.title.trim() : "", DEDUPE_DEBOUNCE_MS);
  const pending = active && isDebouncing(input.title.trim(), term);
  const enabled = active && !pending;
  const query = useDuplicateSearch(input.workspaceSlug, input.projectId, term, enabled);
  if (!active) return NO_MATCHES;
  // While the next term is still settling the previous result is stale;
  // hide it rather than showing duplicates for words no longer typed.
  const matches = pending ? [] : selectDuplicateMatches(query.data ?? [], term, input.projectId);
  return {
    matches,
    isLoading: pending || query.isFetching,
    isEnabled: enabled && matches.length > 0,
  };
}

/** True while the typed term has not settled into the debounced one. */
export function isDebouncing(term: string, debounced: string): boolean {
  return term !== debounced;
}

/**
 * The count button and side-by-side panel the cloud seam feeds (ISS-121).
 * An empty result renders nothing, so the community build shows no trace
 * of the feature; matches collapse behind a count until asked for.
 */
export function DuplicatePanel({ matches }: { matches: readonly IssueSearchHit[] }): React.ReactElement | null {
  const [open, setOpen] = React.useState(false);
  if (matches.length === 0) return null;
  return React.createElement(
    "div",
    { className: "flex flex-col items-start gap-(--space-2)" },
    React.createElement(
      "button",
      {
        type: "button",
        "aria-expanded": open ? "true" : "false",
        onClick: () => setOpen((current) => !current),
        className:
          "inline-flex h-(--control-height-sm) items-center rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-3) text-meta text-(--text) hover:bg-(--subtle) focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-(--focus-ring-color) focus-visible:outline-offset-1",
      },
      `${matches.length} possible duplicate${matches.length === 1 ? "" : "s"}`
    ),
    open
      ? React.createElement(
          "aside",
          { "aria-label": "Possible duplicates", className: "flex flex-col gap-(--space-1)" },
          React.createElement(
            "ul",
            { className: "flex flex-col gap-(--space-1)" },
            matches.map((match) =>
              React.createElement(
                "li",
                { key: match.id, className: "text-body" },
                React.createElement(
                  "a",
                  {
                    href: `/${match.workspace__slug}/browse/${match.project__identifier}-${match.sequence_id}`,
                    target: "_blank",
                    rel: "noreferrer",
                    className: "text-(--text) underline-offset-2 hover:underline",
                  },
                  match.name
                )
              )
            )
          )
        )
      : null
  );
}

/** Whether the title is long enough to spend a search on it. */
export function shouldSearchDuplicates(title: string, enabled: boolean): boolean {
  return enabled && title.trim().length >= 3;
}
