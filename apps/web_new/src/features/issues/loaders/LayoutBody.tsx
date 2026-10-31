// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Layout body router (parity/specs/issues.md §1, ISS-004). Routes between
// the empty states and the layout content by issue counts: a zero total
// renders the context empty state, zero visible rows with a nonzero total
// renders the no-match state, and calendar always renders its grid (it
// represents emptiness per day). The initial-load skeleton lives in the
// route pending component, not here.

import * as React from "react";

import type { LayoutKind } from "../layouts/kinds.js";

export interface LayoutBodyProps {
  layout: LayoutKind;
  /** Server total across all pages. */
  totalCount: number;
  /** Rows visible after filtering. */
  resultCount: number;
  /** Zero-total empty state (context copy arrives with ISS-068). */
  emptyState: React.ReactNode;
  /** Filtered-to-zero empty state. */
  noMatchState: React.ReactNode;
  children: React.ReactNode;
}

export function LayoutBody({
  layout,
  totalCount,
  resultCount,
  emptyState,
  noMatchState,
  children,
}: LayoutBodyProps): React.ReactElement {
  if (totalCount === 0 && layout !== "calendar") return <>{emptyState}</>;
  if (resultCount === 0 && layout !== "calendar") return <>{noMatchState}</>;
  return <>{children}</>;
}
