// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Heading-density preview (parity/specs/pages.md §6.5, PAGE-046). While the
// navigation pane is shut, a slim set of bars sits at the canvas edge, one
// per heading, so the document's shape stays visible. Hovering reveals the
// heading texts; picking one opens the pane on the outline and goes there.

import * as React from "react";

import { pagePreviewBars, type PageHeading } from "../lib/pagePane.js";

export const PAGE_PREVIEW_OPEN_LABEL = "Open outline";

export interface PageHeadingPreviewProps {
  headings: readonly PageHeading[];
  /** Open the outline, optionally at one heading. */
  onOpen: (heading: PageHeading | null) => void;
}

export function PageHeadingPreview({ headings, onOpen }: PageHeadingPreviewProps): React.ReactElement | null {
  // A document without headings has no shape to preview; the toolbar
  // control is the only way in (PAGE-046).
  if (headings.length === 0) return null;
  const bars = pagePreviewBars(headings);
  return (
    <div data-testid="page-heading-preview" className="group absolute top-28 right-0 z-10">
      <button
        type="button"
        aria-label={PAGE_PREVIEW_OPEN_LABEL}
        onClick={() => onOpen(null)}
        className="flex cursor-pointer flex-col items-end gap-1.5 rounded-l-(--radius-control) border border-r-0 border-(--border) bg-(--bg) px-(--space-2) py-(--space-3) shadow-(--shadow-popover) hover:bg-(--subtle)"
      >
        {bars.map((bar, index) => (
          <div
            key={`${bar.level}-${index}`}
            aria-hidden="true"
            style={{ width: `${bar.widthPx}px` }}
            className={bar.level === 1 ? "h-0.5 rounded-full bg-(--text-muted)" : "h-0.5 rounded-full bg-(--border)"}
          />
        ))}
      </button>
      <div className="pointer-events-none absolute top-0 right-full mr-(--space-2) w-56 rounded-(--radius-card) border border-(--border) bg-(--bg) p-(--space-2) opacity-0 shadow-(--shadow-popover) transition-opacity duration-(--motion-duration) group-focus-within:pointer-events-auto group-focus-within:opacity-100 group-hover:pointer-events-auto group-hover:opacity-100">
        <ul className="flex max-h-80 flex-col overflow-y-auto">
          {headings.map((heading) => (
            <li key={`${heading.level}-${heading.levelIndex}`}>
              <button
                type="button"
                onClick={() => onOpen(heading)}
                style={{ paddingLeft: `${heading.level * 8}px` }}
                className="text-body block w-full cursor-pointer truncate rounded-(--radius-control) px-(--space-2) py-(--space-1) text-left text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
              >
                {heading.text === "" ? "Untitled heading" : heading.text}
              </button>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
