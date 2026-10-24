// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public-board message catalog (parity/specs/space.md SPC-065). Every
// string the space chrome renders — navbar, badge, account controls,
// loaders, failure surfaces, progress and notices — is read through
// spaceMessage by a stable key, never inlined at the call site. The
// open build ships English only; fuller builds register per-tag packs
// and the selector prefers the pack entry, falling back to English
// key by key, which is why keys are stable identifiers rather than
// English text. Server data (project, state and member names,
// identifiers, anchors) and date shapes never pass through here.
// Two notice pairs reproduce the oracle's pinned wording exactly: the
// parity suite asserts them on the new app, so they are the specified
// contract, sourced from the oracle scenarios rather than lifted from
// old components. Every other entry is new wording.
import { readStoredLanguage } from "../../../core/i18n/locales.js";

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
  "loading.board",
  "loading.legacy",
  "loading.instance",
  "board.issuesFallback",
  "failure.anchor.docTitle",
  "failure.anchor.heading",
  "failure.anchor.body",
  "failure.anchor.art",
  "failure.board.docTitle",
  "failure.board.heading",
  "failure.board.body",
  "failure.board.art",
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
  "loading.board": "Loading board",
  "loading.legacy": "Resolving board",
  "loading.instance": "Loading instance",
  "board.issuesFallback": "Issues",
  "failure.anchor.docTitle": "Pi Dash — Board not found",
  "failure.anchor.heading": "We couldn't find this board",
  "failure.anchor.body": "The board may have been withdrawn or the address may be mistyped.",
  "failure.anchor.art": "An empty notice board",
  "failure.board.docTitle": "Pi Dash — Board error",
  "failure.board.heading": "We couldn't load this board",
  "failure.board.body": "The board data did not arrive. Reload the page or come back in a moment.",
  "failure.board.art": "An unavailable server",
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

/** A locale pack: translated entries for a subset of the stable keys. */
export type SpaceMessagePack = Partial<Record<SpaceMessageKey, string>>;

const packs = new Map<string, SpaceMessagePack>();

/**
 * Register a locale pack (fuller builds call this at bootstrap; the
 * open build never does). Re-registering a tag merges into it, so
 * packs can arrive in pieces.
 */
export function registerSpaceMessages(tag: string, pack: SpaceMessagePack): void {
  packs.set(tag, { ...packs.get(tag), ...pack });
}

/** Drop test-only registrations (tests only; the app never unregisters). */
export function resetSpaceMessagesForTests(): void {
  packs.clear();
}

/**
 * The text for a stable message key: the registered pack entry for the
 * tag wins, English stays the fallback. The tag defaults to the stored
 * subtree language, which the boot gate heals to a supported tag.
 */
export function spaceMessage(key: SpaceMessageKey, tag: string = readStoredLanguage()): string {
  return packs.get(tag)?.[key] ?? ENGLISH[key];
}
