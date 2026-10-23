// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public-board message table (parity/specs/space.md SPC-065). Every
// string the space chrome renders — navbar, badge, account controls,
// progress and notices — is read through spaceMessage by a stable key,
// never inlined at the call site. The open build ships English only;
// cloud locale packs extend this module with a per-tag selector, which
// is why keys are stable identifiers rather than English text. Server
// data (project, state and member names, identifiers, anchors) and
// date shapes never pass through here.
// Two notice pairs reproduce the oracle's pinned wording exactly: the
// parity suite asserts them on the new app, so they are the specified
// contract, sourced from the oracle scenarios rather than lifted from
// old components. Every other entry is new wording.

/** Stable identifiers for every public-chrome string. */
export const SPACE_MESSAGE_KEYS = [
  "theme.switchToDark",
  "theme.switchToLight",
  "account.signIn",
  "account.signOut",
  "account.fallbackName",
  "badge.lead",
  "badge.product",
  "progress.loading",
  "notice.copySuccess.title",
  "notice.copySuccess.body",
  "notice.copyFailure.title",
  "notice.copyFailure.body",
  "notice.commentFailure.title",
  "notice.commentFailure.body",
  "notice.voteFailure.title",
  "notice.voteFailure.body",
  "notice.reactionFailure.title",
  "notice.reactionFailure.body",
  "notice.commentEditFailure.title",
  "notice.commentEditFailure.body",
  "notice.commentDeleteSuccess.title",
  "notice.commentDeleteFailure.title",
  "notice.commentDeleteFailure.body",
  "notice.attachmentFailure.title",
  "notice.attachmentFailure.body",
] as const;

export type SpaceMessageKey = (typeof SPACE_MESSAGE_KEYS)[number];

const ENGLISH: Record<SpaceMessageKey, string> = {
  "theme.switchToDark": "Switch to dark theme",
  "theme.switchToLight": "Switch to light theme",
  "account.signIn": "Sign in",
  "account.signOut": "Sign out",
  "account.fallbackName": "Account",
  "badge.lead": "Built with",
  "badge.product": "Pi Dash Publish",
  "progress.loading": "Loading",
  // Pinned by the parity suite (board-chrome.spec.ts, SPC-017): the exact
  // wording a copy confirmation must carry on the new app.
  "notice.copySuccess.title": "Link copied!",
  "notice.copySuccess.body": "Work item link copied to clipboard.",
  "notice.copyFailure.title": "Couldn't copy the link",
  "notice.copyFailure.body": "The clipboard refused the write — copy the address manually.",
  // Pinned by the parity suite (board-chrome.spec.ts, SPC-017): the exact
  // wording a failed comment post must carry on the new app.
  "notice.commentFailure.title": "Error!",
  "notice.commentFailure.body": "Comment could not be posted. Please try again.",
  "notice.voteFailure.title": "Error!",
  "notice.voteFailure.body": "Your vote could not be saved. Please try again.",
  "notice.reactionFailure.title": "Error!",
  "notice.reactionFailure.body": "Your reaction could not be saved. Please try again.",
  "notice.commentEditFailure.title": "Error!",
  "notice.commentEditFailure.body": "Your edit could not be saved. Please try again.",
  "notice.commentDeleteSuccess.title": "Comment deleted!",
  "notice.commentDeleteFailure.title": "Error!",
  "notice.commentDeleteFailure.body": "The comment could not be deleted. Please try again.",
  "notice.attachmentFailure.title": "Couldn't attach the file",
  "notice.attachmentFailure.body": "The upload failed. Please try again.",
};

/** The English text for a stable message key. */
export function spaceMessage(key: SpaceMessageKey): string {
  return ENGLISH[key];
}
