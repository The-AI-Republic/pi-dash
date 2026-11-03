// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the shared editor: the read surface, the lazy editing
// surface, and the HTML helpers around stored rich text.

export { loadCommentExtensions, loadDescriptionExtensions, loadIssueDescriptionExtensions } from "./extensions.js";
export { CommentRichTextEditor, IssueDescriptionEditor, RichTextEditor } from "./RichTextEditor.js";
export {
  alignmentOf,
  blockKindForHeading,
  EMPTY_FORMATTING_STATE,
  formattingStatesEqual,
  headingLevelOf,
  readFormattingState,
} from "./pageFormatting.js";
export type {
  PageAlignment,
  PageBlockKind,
  PageEditorProbe,
  PageFormatCommand,
  PageFormattingState,
  PageListKind,
} from "./pageFormatting.js";
export { loadPageExtensions } from "./pageExtensions.js";
export { MENTION_CONTENT_TAGS, MENTION_ENTITY, MENTION_TAG, mentionAttributes, mentionQueryAt } from "./mention.js";
export type { MentionQuery, MentionTarget } from "./mention.js";
export type { RichTextEditorHandle, RichTextEditorProps, StoredImageInsert } from "./RichTextEditor.js";
export { RichTextView } from "./RichTextView.js";
export type { RichTextViewProps } from "./RichTextView.js";
export {
  descriptionAssetSrc,
  hasStoredDescriptionContent,
  isStoredHtmlEmpty,
  isStoredHtmlEmptyIgnoring,
  mentionDisplayName,
  prepareEditorHtml,
  renderableHtml,
  sanitizeHtml,
  STORED_CONTENT_TAGS,
  storedImageSrc,
  storedMentionOf,
  storedTextContent,
} from "./sanitize.js";
export type { DescriptionAssetScope, StoredMention } from "./sanitize.js";
