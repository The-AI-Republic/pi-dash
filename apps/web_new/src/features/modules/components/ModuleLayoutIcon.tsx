// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Glyph for each modules-list layout (parity/specs/modules.md §3.4,
// MOD-011). Decorative everywhere: the switcher button and the menu entry
// carry the accessible name.

import { ChartGantt, LayoutGrid, Rows3 } from "lucide-react";
import * as React from "react";

import type { ModulesLayout } from "../lib/moduleLayouts.js";

export function ModuleLayoutIcon({ layout }: { layout: ModulesLayout }): React.ReactElement {
  const Glyph = layout === "board" ? LayoutGrid : layout === "timeline" ? ChartGantt : Rows3;
  return <Glyph size={16} strokeWidth={1.5} aria-hidden="true" />;
}
