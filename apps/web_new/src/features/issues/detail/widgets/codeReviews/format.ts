// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Code-review presentation rules (parity/specs/issues.md §16, ISS-205).
// The badge collapses the server's three states plus the draft marker
// into the four labels the section renders; the section stays hidden
// until a review exists, so an empty collection adds no chrome.

import type { CodeReviewLink } from "@pidash/api-client";

export type ReviewBadge = "Merged" | "Draft" | "Open" | "Closed";

/** Badge label for one link: merged wins, then draft, then state. */
export function reviewBadge(link: Pick<CodeReviewLink, "merged" | "draft" | "state">): ReviewBadge {
  if (link.merged || link.state === "merged") return "Merged";
  if (link.draft) return "Draft";
  if (link.state === "open") return "Open";
  return "Closed";
}

/** The link's display title, falling back to its URL when unset. */
export function reviewTitle(link: Pick<CodeReviewLink, "title" | "url">): string {
  const title = link.title.trim();
  return title === "" ? link.url : title;
}

/** Section heading, shared by the header and the empty-state check. */
export const CODE_REVIEWS_TITLE = "Code reviews";
/** Input affordance pinned by the parity scenario. */
export const CODE_REVIEW_URL_PLACEHOLDER = "Paste a pull request or merge request URL";
/** Success notice after a link is attached. */
export const CODE_REVIEW_ATTACHED_TITLE = "Code review attached";
/** Failure notice when the server refuses the URL. */
export const CODE_REVIEW_ATTACH_FAILED_TITLE = "Code review not attached";
/** Success notice after a link is detached. */
export const CODE_REVIEW_DETACHED_TITLE = "Code review detached";
/** Failure notice when a detach is refused. */
export const CODE_REVIEW_DETACH_FAILED_TITLE = "Couldn't detach the code review";
