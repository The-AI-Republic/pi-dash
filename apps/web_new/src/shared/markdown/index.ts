// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Markdown-to-HTML for model replies (UI kit and editor page). Assistant
// answers arrive as markdown and are shown read-only, so this renders a
// small, safe subset: raw HTML is never interpreted, links open away from
// the app, and the result is sanitized before it reaches the DOM.
//
// The renderer and the sanitizer are loaded on first use. Their setup is a
// module-level side effect, which would otherwise keep this module (and
// both libraries) in every bundle that merely reaches the assistant's
// public entry, including the first-paint chunk.

/** Renders one markdown source to sanitized HTML. */
export type RenderMarkdown = (source: string) => string;

export const SANITIZE_OPTIONS = {
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

async function buildRenderer(): Promise<RenderMarkdown> {
  const [{ default: MarkdownIt }, { default: DOMPurify }] = await Promise.all([
    import("markdown-it"),
    import("dompurify"),
  ]);
  const engine = new MarkdownIt({ html: false, linkify: true, breaks: true });
  const defaultLinkOpen =
    engine.renderer.rules.link_open ??
    ((tokens, index, options, _env, self) => self.renderToken(tokens, index, options));
  engine.renderer.rules.link_open = (tokens, index, options, env, self) => {
    const token = tokens[index];
    if (token !== undefined) {
      token.attrSet("target", "_blank");
      token.attrSet("rel", "noopener noreferrer");
    }
    return defaultLinkOpen(tokens, index, options, env, self);
  };
  return (source: string) => DOMPurify.sanitize(engine.render(source), SANITIZE_OPTIONS);
}

let renderer: Promise<RenderMarkdown> | null = null;

/** The shared renderer, built once on first use. */
export function loadMarkdown(): Promise<RenderMarkdown> {
  renderer ??= buildRenderer();
  return renderer;
}

/** Render model markdown to sanitized HTML for a read-only surface. */
export async function markdownHtml(source: string): Promise<string> {
  const render = await loadMarkdown();
  return render(source);
}
