// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Manage-widgets dialog rules (SHELL-018–019; written from
// parity/specs/shell.md §2). The dialog lists the surfaced widgets — the
// sections the stack can render — so the retired key and any other
// non-rendered key never appear as a toggle. Reordering patches the
// dragged widget alone: onto the top it takes a sort above every known
// row, otherwise one below the row it lands under. A single integer
// patch cannot split two adjacent sorts, so a middle drop across a
// one-wide gap ties with its lower neighbor; only two surfaced rows
// exist, so every real drop lands on an end and stays exact.

import type { HomePreference, HomePreferenceList, HomeWidgetKey } from "@pidash/api-client";

import { orderWidgetPrefs, widgetSectionKind } from "./widgets.js";

const WIDGET_DISPLAY_NAMES: Record<string, string> = {
  quick_links: "Quicklinks",
  recents: "Recents",
};

/** The dialog rows: surfaced widgets in stack order, highest position first. */
export function surfacedWidgetPrefs(prefs: HomePreferenceList): HomePreferenceList {
  return orderWidgetPrefs(prefs.filter((pref) => widgetSectionKind(pref.key) !== "other"));
}

export function widgetDisplayName(key: string): string {
  return WIDGET_DISPLAY_NAMES[key] ?? key;
}

export interface WidgetReorder {
  key: HomeWidgetKey;
  sort_order: number;
}

/**
 * The patch moving `key` to `toIndex` in the surfaced order, or null when
 * the drop changes nothing (unknown key, a single row, or the row's own
 * slot). Positions clamp into the list.
 */
export function planWidgetReorder(
  prefs: HomePreferenceList,
  key: HomeWidgetKey,
  toIndex: number
): WidgetReorder | null {
  const surfaced = surfacedWidgetPrefs(prefs);
  const [dragged] = surfaced.filter((pref) => pref.key === key);
  if (dragged === undefined) return null;
  if (surfaced.length < 2) return null;
  const from = surfaced.indexOf(dragged);
  const clamped = Math.min(Math.max(toIndex, 0), surfaced.length - 1);
  if (from === clamped) return null;

  const rest = surfaced.filter((pref) => pref.key !== key);
  const next: HomePreferenceList = [...rest.slice(0, clamped), dragged, ...rest.slice(clamped)];
  let above: HomePreference | undefined;
  let previous: HomePreference | undefined;
  for (const pref of next) {
    if (pref.key === key) {
      above = previous;
      break;
    }
    previous = pref;
  }
  if (above === undefined) {
    let top = Number.NEGATIVE_INFINITY;
    for (const pref of prefs) top = Math.max(top, pref.sort_order);
    return { key, sort_order: top + 1 };
  }
  return { key, sort_order: above.sort_order - 1 };
}
