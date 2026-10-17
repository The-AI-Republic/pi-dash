// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Desktop issues header (parity/specs/issues.md §1, ISS-001). Title and
// count plus the layout switcher; the trailing slot hosts the Display,
// filter and analytics controls their slices add. Hidden under md, where
// the compact mobile header takes over.

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
}

export function IssuesHeader({
  title,
  totalCount,
  offered,
  active,
  onSwitch,
  actions,
}: IssuesHeaderProps): React.ReactElement {
  return (
    <div
      data-testid="issues-header"
      className="hidden shrink-0 items-center gap-(--space-3) border-b border-(--border) px-(--space-4) py-(--space-3) md:flex"
    >
      <h2 className="text-h3 text-(--text)">{title}</h2>
      <span className="text-body text-(--text-muted)">
        {totalCount} issue{totalCount === 1 ? "" : "s"}
      </span>
      <div className="ml-auto flex items-center gap-(--space-2)">
        {actions}
        <LayoutSwitcher offered={offered} active={active} onSwitch={onSwitch} />
      </div>
    </div>
  );
}
