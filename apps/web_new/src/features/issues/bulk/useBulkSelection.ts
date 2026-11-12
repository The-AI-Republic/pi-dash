// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Bulk selection hook (parity/specs/issues.md §10, ISS-108/109/110).
// Owns the selected ids, the range anchor and the keyboard-active row.
// Arrow keys move the active row and scroll it into view; Shift plus an
// arrow adds the adjacent row. Keystrokes inside form controls are left
// alone. An active selection arms the browser's leave confirmation and
// the hook clears itself on unmount, which is what a confirmed leave
// ends up doing anyway.

import * as React from "react";

import {
  extendSelection,
  groupSelectionState,
  moveActive,
  pruneSelection,
  toggleGroup,
  toggleItem,
  type GroupSelectionState,
  type SelectionItem,
} from "./model.js";

/** Attribute the hook uses to find a row's DOM node for scrolling. */
export const SELECTION_ROW_ATTR = "data-selection-id";

/** Root attribute while a live selection arms the leave confirmation (ISS-110). */
export const LEAVE_GUARD_ATTR = "data-leave-guard";

export interface BulkSelection {
  selectedIds: ReadonlySet<string>;
  activeId: string | null;
  isSelected: (id: string) => boolean;
  isActive: (id: string) => boolean;
  /** Toggle one row; shift-click extends from the anchor. */
  onRowToggle: (id: string, shiftKey: boolean) => void;
  /** Toggle a whole group (tri-state header). */
  onGroupToggle: (groupKey: string) => void;
  groupState: (groupKey: string) => GroupSelectionState;
  /** Clear the selection and the active row. */
  clear: () => void;
}

function isFormTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  const tag = target.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || target.isContentEditable;
}

/** Scroll the active row into view without moving the page. */
function revealRow(id: string, reveal?: ((id: string) => void) | undefined): void {
  if (reveal !== undefined) {
    reveal(id);
    return;
  }
  const node = document.querySelector(`[${SELECTION_ROW_ATTR}="${CSS.escape(id)}"]`);
  if (node !== null && typeof node.scrollIntoView === "function") node.scrollIntoView({ block: "nearest" });
}

export function useBulkSelection(
  items: readonly SelectionItem[],
  enabled = true,
  /** Surface-owned reveal for rows outside a virtualized window. */
  reveal?: ((id: string) => void) | undefined
): BulkSelection {
  const [selectedIds, setSelectedIds] = React.useState<ReadonlySet<string>>(() => new Set());
  const [activeId, setActiveId] = React.useState<string | null>(null);
  const anchorRef = React.useRef<string | null>(null);
  const revealRef = React.useRef(reveal);
  revealRef.current = reveal;
  const itemsRef = React.useRef(items);
  itemsRef.current = items;

  // Auto-prune entities that leave the list on a filter or regroup.
  React.useEffect(() => {
    setSelectedIds((current) => {
      const next = pruneSelection(current, items);
      if (next.size === current.size && [...current].every((id) => next.has(id))) return current;
      return next;
    });
    setActiveId((current) => {
      if (current === null) return null;
      const present = items.some((item) => item.id === current);
      if (!present) return null;
      return current;
    });
  }, [items]);

  React.useEffect(() => {
    if (!enabled) return;
    const handleKeyDown = (event: KeyboardEvent): void => {
      if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
      if (isFormTarget(event.target)) return;
      const direction = event.key === "ArrowDown" ? "down" : "up";
      if (event.shiftKey) {
        setSelectedIds((current) => extendSelection(current, activeId, itemsRef.current, direction));
        setActiveId((current) => moveActive(current, itemsRef.current, direction));
        const target = moveActive(activeId, itemsRef.current, direction);
        if (target !== null) revealRow(target, revealRef.current);
      } else {
        const target = moveActive(activeId, itemsRef.current, direction);
        setActiveId(target);
        if (target !== null) revealRow(target, revealRef.current);
      }
      event.preventDefault();
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [activeId, enabled]);

  React.useEffect(() => {
    if (selectedIds.size === 0) return;
    const handleBeforeUnload = (event: BeforeUnloadEvent): void => {
      event.preventDefault();
      event.returnValue = "";
    };
    window.addEventListener("beforeunload", handleBeforeUnload, true);
    // A headless browser does not raise the native prompt without user
    // activation, so the armed state is published on the root element
    // where a scenario (and a reader) can observe it directly.
    const root = document.documentElement;
    root.setAttribute(LEAVE_GUARD_ATTR, "armed");
    return () => {
      window.removeEventListener("beforeunload", handleBeforeUnload, true);
      root.removeAttribute(LEAVE_GUARD_ATTR);
    };
  }, [selectedIds.size]);

  const clear = React.useCallback(() => {
    setSelectedIds(new Set());
    setActiveId(null);
    anchorRef.current = null;
  }, []);

  const onRowToggle = React.useCallback((id: string, shiftKey: boolean) => {
    setSelectedIds((current) => {
      const next = toggleItem(current, itemsRef.current, id, anchorRef.current, shiftKey);
      if (!next.has(id) && current.has(id)) {
        anchorRef.current = null;
      } else {
        anchorRef.current = id;
      }
      return next;
    });
    setActiveId(id);
  }, []);

  const onGroupToggle = React.useCallback((groupKey: string) => {
    setSelectedIds((current) => toggleGroup(current, itemsRef.current, groupKey));
  }, []);

  const groupState = React.useCallback(
    (groupKey: string) => (enabled ? groupSelectionState(selectedIds, itemsRef.current, groupKey) : "empty"),
    [enabled, selectedIds]
  );

  return {
    selectedIds,
    activeId,
    isSelected: (id) => selectedIds.has(id),
    isActive: (id) => activeId === id,
    onRowToggle,
    onGroupToggle,
    groupState,
    clear,
  };
}
