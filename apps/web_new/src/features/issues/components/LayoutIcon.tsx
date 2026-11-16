// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Layout glyphs for the switcher and the mobile layout menu (ISS-001).

import { CalendarDays, ChartGantt, List, SquareKanban, Table } from "lucide-react";
import * as React from "react";

import type { LayoutKind } from "../layouts/kinds.js";

const GLYPHS: Record<LayoutKind, typeof List> = {
  list: List,
  kanban: SquareKanban,
  calendar: CalendarDays,
  spreadsheet: Table,
  gantt_chart: ChartGantt,
};

export interface LayoutIconProps {
  kind: LayoutKind;
  size?: number;
}

/** The glyph for one layout kind; always hidden from assistive tech. */
export function LayoutIcon({ kind, size = 16 }: LayoutIconProps): React.ReactElement {
  const Glyph = GLYPHS[kind];
  return <Glyph size={size} strokeWidth={1.5} aria-hidden="true" />;
}
