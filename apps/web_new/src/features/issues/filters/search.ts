// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Issue view search (Data layer > Routing). The list route validates its
// single search param against this schema; anything invalid falls back to
// the defaults instead of throwing, so shared links never break.

import * as z from "zod/mini";

export const IssueLayout = z.enum(["list", "compact"]);
export type IssueLayout = z.infer<typeof IssueLayout>;

export const IssueSearch = z.object({
  layout: z.optional(z._default(IssueLayout, "list")),
});
export type IssueSearch = z.infer<typeof IssueSearch>;

export const DEFAULT_ISSUE_SEARCH: IssueSearch = { layout: "list" };

/** Validate raw router search; invalid params fall back to defaults. */
export function parseIssueSearch(raw: unknown): IssueSearch {
  try {
    return IssueSearch.parse(raw ?? {});
  } catch {
    return { ...DEFAULT_ISSUE_SEARCH };
  }
}
