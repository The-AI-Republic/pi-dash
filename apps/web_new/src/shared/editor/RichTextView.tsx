// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Read-only stored-HTML render (UI kit and editor page). No Tiptap:
// lists, peek, comments and activity show descriptions through this
// and load the editor chunk only when the user starts editing.

import * as React from "react";

import { sanitizeHtml } from "./sanitize.js";

export interface RichTextViewProps {
  /** Stored description or comment HTML. */
  html: string;
  /** Accessible label for the rendered region. */
  label: string;
  className?: string | undefined;
}

export function RichTextView({ html, label, className }: RichTextViewProps): React.ReactElement {
  const clean = React.useMemo(() => sanitizeHtml(html), [html]);
  const classes = className === undefined ? "rich-text-content" : `rich-text-content ${className}`;
  return <div aria-label={label} role="region" className={classes} dangerouslySetInnerHTML={{ __html: clean }} />;
}
