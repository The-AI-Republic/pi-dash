// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the shared editor: the read surface, the lazy editing
// surface, and the HTML helpers around stored rich text.

export { loadCommentExtensions, loadDescriptionExtensions } from "./extensions.js";
export { CommentRichTextEditor, RichTextEditor } from "./RichTextEditor.js";
export type { RichTextEditorHandle, RichTextEditorProps } from "./RichTextEditor.js";
export { RichTextView } from "./RichTextView.js";
export type { RichTextViewProps } from "./RichTextView.js";
export { isStoredHtmlEmpty, isStoredHtmlEmptyIgnoring, sanitizeHtml, storedTextContent } from "./sanitize.js";
