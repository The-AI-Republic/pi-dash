// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public-board message catalog (parity/specs/space.md SPC-065). Every
// string the space chrome renders — navbar, badge, account controls,
// filters, loaders, failure surfaces, progress and notices — is read through
// spaceMessage by a stable key, never inlined at the call site. The
// open build ships English only; fuller builds register per-tag packs
// and the selector prefers the pack entry, falling back to English
// key by key, which is why keys are stable identifiers rather than
// English text. Server data (project, state and member names,
// identifiers, anchors) and date shapes never pass through here.
// Every entry is new wording: notice copy is not a parity property,
// so the parity suite pins each outcome's intent (a success notice
// after copying the link, an error notice after a failed post),
// never the sentences below.
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
  "filters.open",
  "filters.search",
  "filters.priority",
  "filters.state",
  "filters.labels",
  "filters.priority.urgent",
  "filters.priority.high",
  "filters.priority.medium",
  "filters.priority.low",
  "filters.priority.none",
  "filters.noMatches",
  "filters.showMore",
  "filters.showLess",
  "filters.removeValue",
  "filters.clearFacet",
  "filters.clearAll",
  "peek.presentation",
  "peek.mode.side",
  "peek.mode.modal",
  "peek.mode.full",
  "peek.copyLink",
  "peek.titleLabel",
  "peek.descriptionLabel",
  "peek.loading",
  "peek.detailFailure",
  "peek.retry",
  "peek.property.state",
  "peek.property.priority",
  "peek.property.dueDate",
  "peek.empty",
  "engagement.label",
  "engagement.you",
  "engagement.noVotes",
  "engagement.chooseReaction",
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
  "failure.anchor.docTitle": "Pi Dash — Board not found",
  "failure.anchor.heading": "We couldn't find this board",
  "failure.anchor.body": "The board may have been withdrawn or the address may be mistyped.",
  "failure.anchor.art": "An empty notice board",
  "failure.board.docTitle": "Pi Dash — Board error",
  "failure.board.heading": "We couldn't load this board",
  "failure.board.body": "The board data did not arrive. Reload the page or come back in a moment.",
  "failure.board.art": "An unavailable server",
  // The parity suite (board-chrome.spec.ts, SPC-017) pins the intent —
  // a success notice naming the copy — never these sentences.
  "notice.copySuccess.title": "Board link copied",
  "notice.copySuccess.body": "The link is on your clipboard, ready to share.",
  "notice.copyFailure.title": "Couldn't copy the link",
  "notice.copyFailure.body": "The clipboard refused the write — copy the address manually.",
  // The parity suite pins the intent — an error notice naming the
  // comment — never these sentences.
  "notice.commentFailure.title": "Couldn't post the comment",
  "notice.commentFailure.body": "Your comment was not posted. Give it another try.",
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
  // Filter labels the parity suite pins (filters.spec.ts, SPC-034/035):
  // these spellings are the board's interface contract, translated
  // through this catalog like every other chrome string.
  "filters.open": "Filters",
  "filters.search": "Search",
  "filters.priority": "Priority",
  "filters.state": "State",
  "filters.labels": "Labels",
  "filters.priority.urgent": "Urgent",
  "filters.priority.high": "High",
  "filters.priority.medium": "Medium",
  "filters.priority.low": "Low",
  "filters.priority.none": "None",
  "filters.noMatches": "No matches found",
  "filters.showMore": "Show more",
  "filters.showLess": "Show less",
  "filters.removeValue": "Remove filter",
  "filters.clearFacet": "Clear facet",
  "filters.clearAll": "Clear all",
  // Public issue overlay (SPC-040–046). The property labels and the empty
  // marker are the panel's interface contract; the notices keep their own
  // existing keys.
  "peek.presentation": "Change presentation",
  "peek.mode.side": "Side panel",
  "peek.mode.modal": "Modal",
  "peek.mode.full": "Full screen",
  "peek.copyLink": "Copy link",
  "peek.titleLabel": "Issue title",
  "peek.descriptionLabel": "Issue description",
  "peek.loading": "Loading issue",
  "peek.detailFailure": "This issue could not be loaded.",
  "peek.retry": "Try again",
  "peek.property.state": "State",
  "peek.property.priority": "Priority",
  "peek.property.dueDate": "Due date",
  "peek.empty": "Empty",
  // Public issue engagement (SPC-047/048). The hints name server-provided
  // voters; only the surrounding words translate through these keys.
  "engagement.label": "Votes and reactions",
  "engagement.you": "you",
  "engagement.noVotes": "No votes yet",
  "engagement.chooseReaction": "Choose a reaction",
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
