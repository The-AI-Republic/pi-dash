// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Row-action catalog (parity/specs/drafts.md §3.1, DRAFT-009). The four
// entries every row opener renders — the hover ellipsis and the
// right-click menu — in pinned order with resolved display labels. One
// table drives both openers so the two never drift, and the internal
// ids are never rendered: the old app showed raw i18n keys here, and
// the new labels are translated text (NEWFRONT-298). The open build
// ships English only (core/i18n manages the locale tag and has no
// message catalog), so the labels are the pinned contract strings.

/** Stable row-action ids. Internal only; never rendered. */
export const DRAFT_ROW_ACTION_IDS = ["edit", "copy", "move", "delete"] as const;

/** One of the catalog's stable row-action ids. */
export type DraftRowActionId = (typeof DRAFT_ROW_ACTION_IDS)[number];

/** One catalog row: the id both openers key on plus its display label. */
export interface DraftRowAction {
  id: DraftRowActionId;
  /** Resolved display label (spec §1.1 pin), never a raw key. */
  label: string;
  /** Destructive entries render in the danger tone. */
  danger: boolean;
}

/** The row actions in pinned menu order. */
export const DRAFT_ROW_ACTIONS: readonly DraftRowAction[] = [
  { id: "edit", label: "Edit", danger: false },
  { id: "copy", label: "Make a copy", danger: false },
  { id: "move", label: "Move to project", danger: false },
  { id: "delete", label: "Delete", danger: true },
];
