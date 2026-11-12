// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Active-tab scrolling for the workspace views tab strip
// (parity/specs/views.md §10, VIEW-033). Pure: given the tab's and the
// strip's horizontal spans, decide whether the tab needs scrolling and
// how it should land. A fully visible tab stays put; a tab far
// off-screen centers itself; a partially visible tab takes the nearest
// edge. Motion honors the reduced-motion preference.

/** Horizontal span of one laid-out element. */
export interface TabSpan {
  left: number;
  right: number;
}

export type TabScrollPlan = { scroll: false } | { scroll: true; inline: "center" | "nearest" };

/** Where the active tab should land inside the strip. */
export function resolveTabScroll(tab: TabSpan, strip: TabSpan): TabScrollPlan {
  if (tab.left >= strip.left && tab.right <= strip.right) return { scroll: false };
  if (tab.right <= strip.left || tab.left >= strip.right) return { scroll: true, inline: "center" };
  return { scroll: true, inline: "nearest" };
}

/** Scroll pacing: instant when reduced motion is preferred, smooth otherwise. */
export function tabScrollBehavior(reducedMotion: { matches: boolean } | null): ScrollBehavior {
  return reducedMotion?.matches === true ? "auto" : "smooth";
}
