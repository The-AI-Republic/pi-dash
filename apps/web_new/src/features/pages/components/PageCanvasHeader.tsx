// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Canvas header (parity/specs/pages.md §5.1–§5.2, PAGE-026–027). The
// block above the body that carries the page's mark and its title, and
// the hover target the mark affordance reveals from. It also holds the
// measure the body shares, so the title, the bar and the text column line
// up on one axis whether or not the page carries a mark.

import * as React from "react";

import { PageDetailTitle, type PageDetailTitleProps } from "./PageDetailTitle.js";
import { PAGE_MARK_AREA, PageMarkButton } from "./PageMarkButton.js";

export interface PageCanvasHeaderProps {
  title: PageDetailTitleProps;
  /** Stored `logo_props` of the page. */
  logoProps: Record<string, unknown>;
  /** Read-only viewers get the mark as static art and no picker. */
  editable: boolean;
  /** The title holds no text: the mark affordance stays pinned. */
  titleEmpty: boolean;
  /** Persist the chosen mark (an empty record clears it). */
  onMarkPick: (draft: Record<string, unknown>) => void;
}

export function PageCanvasHeader({
  title,
  logoProps,
  editable,
  titleEmpty,
  onMarkPick,
}: PageCanvasHeaderProps): React.ReactElement {
  return (
    <div data-testid="page-canvas-header" className={`${PAGE_MARK_AREA} group/page-header`}>
      <div className="w-full">
        <div className="flex h-12 items-end">
          <PageMarkButton logoProps={logoProps} editable={editable} titleEmpty={titleEmpty} onPick={onMarkPick} />
        </div>
        <PageDetailTitle {...title} />
      </div>
    </div>
  );
}
