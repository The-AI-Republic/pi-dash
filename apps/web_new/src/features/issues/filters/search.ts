// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
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
