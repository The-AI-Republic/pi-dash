// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Outline tab (parity/specs/pages.md §6.6, PAGE-047). The headings of a
// live document in reading order, indented by level so the structure
// reads at a glance; selecting one scrolls the canvas to it. A document
// without headings gets a themed prompt instead of an empty list.

import { EmptyState } from "@pidash/kit";
import * as React from "react";

import { PAGE_OUTLINE_INDENT_PX, type PageHeading, type PageHeadingLevel } from "../lib/pagePane.js";

export const PAGE_OUTLINE_EMPTY_TITLE = "Missing headings";
export const PAGE_OUTLINE_EMPTY_BODY = "Add headings to this page and they will appear here.";
export const PAGE_OUTLINE_EMPTY_ART_ALT = "depicts the outline of a page";

export interface PageOutlinePanelProps {
  headings: readonly PageHeading[];
  onSelectHeading: (heading: PageHeading) => void;
}

/**
 * Themed prompt a document without headings shows (PAGE-047). The art is
 * a custom node so its accessible name describes the outline it depicts
 * (the kit's themed art is decorative by default).
 */
function PageOutlineEmpty(): React.ReactElement {
  return (
    <div data-testid="page-outline-empty">
      <EmptyState
        variant="detailed"
        size="small"
        art={
          <img
            src="/empty-state/general/empty-light.svg"
            alt={PAGE_OUTLINE_EMPTY_ART_ALT}
            className="h-32 w-32"
            draggable={false}
          />
        }
        title={PAGE_OUTLINE_EMPTY_TITLE}
        description={PAGE_OUTLINE_EMPTY_BODY}
      />
    </div>
  );
}

export function PageOutlinePanel({ headings, onSelectHeading }: PageOutlinePanelProps): React.ReactElement {
  if (headings.length === 0) return <PageOutlineEmpty />;
  return (
    <ul data-testid="page-outline" className="flex flex-col gap-px">
      {headings.map((heading) => (
        <li key={`${heading.level}-${heading.levelIndex}`} className="min-w-0">
          <button
            type="button"
            onClick={() => onSelectHeading(heading)}
            style={{ paddingLeft: `${PAGE_OUTLINE_INDENT_PX[heading.level as PageHeadingLevel]}px` }}
            className="text-body block w-full cursor-pointer truncate rounded-(--radius-control) py-(--space-1) pr-(--space-2) text-left text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
          >
            {heading.text === "" ? "Untitled heading" : heading.text}
          </button>
        </li>
      ))}
    </ul>
  );
}
