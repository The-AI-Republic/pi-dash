// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Markdown-to-HTML for model replies (UI kit and editor page). Assistant
// answers arrive as markdown and are shown read-only, so this renders a
// small, safe subset: raw HTML is never interpreted, links open away from
// the app, and the result is sanitized before it reaches the DOM.

import DOMPurify from "dompurify";
import MarkdownIt from "markdown-it";

const engine = new MarkdownIt({
  html: false,
  linkify: true,
  breaks: true,
});

const defaultLinkOpen =
  engine.renderer.rules.link_open ?? ((tokens, index, options, _env, self) => self.renderToken(tokens, index, options));

engine.renderer.rules.link_open = (tokens, index, options, env, self) => {
  const token = tokens[index];
  if (token !== undefined) {
    token.attrSet("target", "_blank");
    token.attrSet("rel", "noopener noreferrer");
  }
  return defaultLinkOpen(tokens, index, options, env, self);
};

const SANITIZE_OPTIONS = {
  ALLOWED_TAGS: [
    "a",
    "blockquote",
    "br",
    "code",
    "em",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "hr",
    "li",
    "ol",
    "p",
    "pre",
    "s",
    "strong",
    "table",
    "tbody",
    "td",
    "th",
    "thead",
    "tr",
    "ul",
  ],
  ALLOWED_ATTR: ["href", "rel", "target"],
};

/** Render model markdown to sanitized HTML for a read-only surface. */
export function markdownHtml(source: string): string {
  return DOMPurify.sanitize(engine.render(source), SANITIZE_OPTIONS);
}
