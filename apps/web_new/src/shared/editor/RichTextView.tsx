// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Read-only stored-HTML render (UI kit and editor page). No Tiptap:
// lists, peek, comments and activity show descriptions through this
// and load the editor chunk only when the user starts editing.

import * as React from "react";

import { renderableHtml } from "./sanitize.js";

export interface RichTextViewProps {
  /** Stored description or comment HTML. */
  html: string;
  /** Accessible label for the rendered region. */
  label: string;
  className?: string | undefined;
  /** Resolve a stored image asset id to its served source (ISS-127). */
  resolveAsset?: ((src: string) => string) | undefined;
}

export function RichTextView({ html, label, className, resolveAsset }: RichTextViewProps): React.ReactElement {
  // Stored bodies carry editor nodes (mentions, image components) that
  // have no paintable markup of their own; the renderable form turns
  // each into a chip or an image before it reaches the frame (ISS-127).
  const clean = React.useMemo(() => renderableHtml(html, resolveAsset), [html, resolveAsset]);
  const classes = className === undefined ? "rich-text-content" : `rich-text-content ${className}`;
  return <div aria-label={label} role="region" className={classes} dangerouslySetInnerHTML={{ __html: clean }} />;
}
