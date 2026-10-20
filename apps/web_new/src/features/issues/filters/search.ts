// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Issue view search (Data layer > Routing). The list route validates its
// single search param against this schema; anything invalid falls back to
// absent instead of throwing, so shared links never break. The layout has
// no schema default on purpose: TanStack writes validated defaults back to
// the URL, and the param must stay out of links unless explicitly chosen
// (AUTH-015/AUTH-016 landings pin clean issue URLs); rendering falls back
// to the list layout where the param is read.

import * as z from "zod/mini";

export const IssueLayout = z.enum(["list", "compact"]);
export type IssueLayout = z.infer<typeof IssueLayout>;

export const IssueSearch = z.object({
  layout: z.optional(IssueLayout),
  /** Open row peek; cleared on close (ISS-013). */
  peekIssueId: z.optional(z.string()),
});
export type IssueSearch = z.infer<typeof IssueSearch>;

/** Validate raw router search; invalid params fall back to absent. */
export function parseIssueSearch(raw: unknown): IssueSearch {
  try {
    return IssueSearch.parse(raw ?? {});
  } catch {
    return {};
  }
}
