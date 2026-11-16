// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Widget-map selectors (SHELL-018–023; written from parity/specs/shell.md
// §2). The home-preferences map answers every widget key with its toggle
// and position; the stack renders the enabled ones in position order,
// highest position first. Only the sections this area owns mount here —
// unknown and retired keys resolve to "other" and render nothing.

import type { HomePreferenceList } from "@pidash/api-client";

/** Widget map in stack order: highest position first. Server order wins ties. */
export function orderWidgetPrefs(prefs: HomePreferenceList): HomePreferenceList {
  return [...prefs].sort((a, b) => b.sort_order - a.sort_order);
}

/** The toggled-on widgets. Toggling itself belongs to NEWFRONT-279. */
export function visibleWidgetPrefs(prefs: HomePreferenceList): HomePreferenceList {
  return prefs.filter((pref) => pref.is_enabled);
}

export type WidgetSectionKind = "recents" | "quicklinks" | "other";

export function widgetSectionKind(key: string): WidgetSectionKind {
  if (key === "recents") return "recents";
  if (key === "quick_links") return "quicklinks";
  return "other";
}
