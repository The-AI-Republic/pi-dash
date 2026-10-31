// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Desktop issues header (parity/specs/issues.md §1, ISS-001). Title and
// count plus the layout switcher; the trailing slot hosts the Display,
// filter and analytics controls their slices add. Hidden under md, where
// the compact mobile header takes over. The title region is the row's
// designated shrinker (min-w-0 + truncate); when the column is narrower
// than the trailing controls even so, the row wraps instead of painting
// under an open sidebar (NEWFRONT-1012).

import * as React from "react";

import type { LayoutKind } from "../layouts/kinds.js";
import { LayoutSwitcher } from "./LayoutSwitcher.js";

export interface IssuesHeaderProps {
  /** Entity title, e.g. the project name. */
  title: string;
  totalCount: number;
  offered: readonly LayoutKind[];
  active: LayoutKind;
  onSwitch: (kind: LayoutKind) => void;
  /** Trailing controls (Display, filters, analytics) from later slices. */
  actions?: React.ReactNode;
  /**
   * Hide the layout switcher while keeping the title, count and actions
   * (a locked saved view renders its stored layout with no switcher,
   * parity/specs/views.md §1).
   */
  hideSwitcher?: boolean | undefined;
}

export function IssuesHeader({
  title,
  totalCount,
  offered,
  active,
  onSwitch,
  actions,
  hideSwitcher = false,
}: IssuesHeaderProps): React.ReactElement {
  return (
    <div
      data-testid="issues-header"
      className="hidden min-w-0 shrink-0 flex-wrap items-center gap-(--space-3) border-b border-(--border) px-(--space-4) py-(--space-3) md:flex"
    >
      <div data-testid="issues-header-title" className="flex min-w-0 flex-1 items-center gap-(--space-3)">
        <h2 className="text-h3 truncate text-(--text)">{title}</h2>
        <span className="text-body shrink-0 text-(--text-muted)">
          {totalCount} issue{totalCount === 1 ? "" : "s"}
        </span>
      </div>
      <div
        data-testid="issues-header-actions"
        className="ml-auto flex min-w-0 flex-wrap items-center justify-end gap-(--space-2)"
      >
        {actions}
        {hideSwitcher ? null : <LayoutSwitcher offered={offered} active={active} onSwitch={onSwitch} />}
      </div>
    </div>
  );
}
