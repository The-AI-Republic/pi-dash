// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page body read seam (parity/specs/pages.md §6.1, §8.1, PAGE-044, 064,
// 074). Export, copy-markdown and the socket-loss save all read the body
// through the live editor with a draft/stored fallback. The live read
// wins only when it carries content, so a mounted but blank handle never
// shadows the visible body (bug NEWFRONT-360).

import { pageBodyMarkdown } from "./pageMarkup.js";

/**
 * True when the body would emit markup: words, pictures, mentions,
 * embeds, rules. A blank serialization ("", "<p></p>") emits nothing.
 */
export function pageBodyHasContent(html: string): boolean {
  return pageBodyMarkdown(html).trim() !== "";
}

/**
 * The body a reader sees: the live editor's HTML when it carries
 * content, otherwise the newest known fallback. A blank fallback stays
 * authoritative — emptying the page records the blank, so falling past
 * it would resurrect stored text the author deleted.
 */
export function preferLiveBodyHtml(liveHtml: string | null | undefined, fallbackHtml: string): string {
  const live = liveHtml ?? null;
  if (live !== null && pageBodyHasContent(live)) return live;
  return fallbackHtml;
}
