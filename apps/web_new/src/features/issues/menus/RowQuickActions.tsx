// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The row's two ways into its menu (parity/specs/issues.md §7, ISS-060):
// a hover-revealed ellipsis on the row itself and the browser's own
// right-click menu, both fed by the same entries. A surface mounts the
// hook once and hands its state to both triggers so the row opens one
// dialog, not two. With no entries at all the triggers stay out of the
// tree, which is also what keeps a read-only row from growing controls.

import { ContextMenu, IconButton, Menu, type MenuEntry } from "@pidash/kit";
import { Ellipsis } from "lucide-react";
import * as React from "react";
import { createPortal } from "react-dom";

import { rowMenuLabel } from "./model.js";
import { quickActionIcon, type QuickActionsState } from "./useQuickActions.js";

/** Entries in the shape both popups render. */
export function menuItemsFor(state: QuickActionsState): MenuEntry[] {
  return state.entries.map((entry) => ({
    label: entry.label,
    itemKey: entry.key,
    icon: quickActionIcon(entry.key),
    disabled: entry.disabled,
    ...(entry.description === undefined ? {} : { description: entry.description }),
    onSelect: () => state.handlers.run(entry.key),
  }));
}

export interface RowMenuButtonProps {
  state: QuickActionsState;
  /** Short key the trigger is announced with, e.g. "WEB-12". */
  issueKey: string;
  /** Extra classes for the trigger wrapper (hover reveal, alignment). */
  className?: string | undefined;
  /**
   * Virtualized rows mount the menu on first interaction: a plain trigger
   * renders at rest and the menu machinery mounts on first hover, click
   * or keyboard activation. A mounted menu per row made every scroll frame
   * that brings rows into view blow the 2,000-row scroll budget.
   */
  defer?: boolean | undefined;
}

/**
 * The hover ellipsis. The kit button carries the accessible name
 * "Actions for WEB-12"; the dialog it opens stays mounted beside it.
 */
export function RowMenuButton({
  state,
  issueKey,
  className,
  defer = false,
}: RowMenuButtonProps): React.ReactElement | null {
  const [active, setActive] = React.useState(false);
  const [open, setOpen] = React.useState(false);
  if (state.entries.length === 0) return null;

  const trigger = (
    <IconButton label={rowMenuLabel(issueKey)} size="small" data-testid="row-menu-trigger">
      <Ellipsis size={16} strokeWidth={1.5} aria-hidden="true" />
    </IconButton>
  );
  const activate = (nextOpen: boolean): void => {
    setOpen(nextOpen);
    setActive(true);
  };

  return (
    <div data-row-menu="true" className={className}>
      {defer && !active ? (
        React.cloneElement(trigger, {
          "aria-haspopup": "menu",
          "aria-expanded": false,
          onPointerEnter: () => setActive(true),
          onPointerDown: (event: React.PointerEvent) => {
            if (event.pointerType === "touch") activate(true);
          },
          onClick: (event: React.MouseEvent) => {
            event.preventDefault();
            activate(true);
          },
          onKeyDown: (event: React.KeyboardEvent) => {
            if (event.key === "Enter" || event.key === " " || event.key === "ArrowDown") {
              event.preventDefault();
              activate(true);
            }
          },
        } as Record<string, unknown>)
      ) : (
        <Menu trigger={trigger} items={menuItemsFor(state)} {...(defer ? { open, onOpenChange: setOpen } : {})} />
      )}
      {createPortal(state.overlays, document.body)}
    </div>
  );
}

export interface RowContextMenuProps {
  state: QuickActionsState;
  /** The row element the right-click lands on. */
  trigger: React.ReactElement<Record<string, unknown>>;
}

/** The same entries on right-click (or the keyboard menu key). */
export function RowContextMenu({ state, trigger }: RowContextMenuProps): React.ReactElement {
  if (state.entries.length === 0) return trigger;
  return <ContextMenu trigger={trigger} items={menuItemsFor(state)} />;
}
