// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { ContextMenu as BaseContextMenu } from "@base-ui-components/react/context-menu";
import * as React from "react";
import { cn } from "../lib/cn";
import { pickDefined } from "../lib/props";
import { isMenuCustom, isMenuSeparator, MenuItemBody, type MenuEntry } from "./Menu";

export interface ContextMenuProps {
  /**
   * Element the menu anchors to. Right-click (or the keyboard menu key)
   * on it opens the menu at the pointer.
   */
  trigger: React.ReactElement<Record<string, unknown>>;
  items: MenuEntry[];
  open?: boolean | undefined;
  onOpenChange?: ((open: boolean) => void) | undefined;
}

const popupClasses =
  "min-w-48 rounded-(--radius-popover) border border-(--border) bg-(--bg) p-(--space-2) shadow-popover";

const itemClasses =
  "flex cursor-default items-center rounded-(--radius-control) px-(--space-4) py-(--space-3) text-body text-(--text) outline-none data-[disabled]:opacity-50 data-[highlighted]:bg-(--subtle)";

export function ContextMenu({ trigger, items, open, onOpenChange }: ContextMenuProps): React.ReactElement {
  return (
    <BaseContextMenu.Root {...pickDefined({ open, onOpenChange })}>
      <BaseContextMenu.Trigger render={trigger} />
      <BaseContextMenu.Portal>
        <BaseContextMenu.Positioner sideOffset={4}>
          <BaseContextMenu.Popup className={popupClasses}>
            {items.map((entry, index) =>
              isMenuSeparator(entry) ? (
                <div
                  key={`separator-${index}`}
                  role="separator"
                  className="mx-(--space-2) my-(--space-2) border-t border-(--border)"
                />
              ) : isMenuCustom(entry) ? (
                <React.Fragment key={entry.entryKey}>{entry.node}</React.Fragment>
              ) : (
                <BaseContextMenu.Item
                  key={entry.itemKey ?? entry.label}
                  disabled={entry.disabled ?? false}
                  onClick={entry.onSelect}
                  className={cn(itemClasses)}
                >
                  {entry.icon === undefined ? null : (
                    <span aria-hidden="true" className="mr-(--space-2) inline-flex shrink-0 items-center">
                      {entry.icon}
                    </span>
                  )}
                  <MenuItemBody label={entry.label} description={entry.description} content={entry.content} />
                </BaseContextMenu.Item>
              )
            )}
          </BaseContextMenu.Popup>
        </BaseContextMenu.Positioner>
      </BaseContextMenu.Portal>
    </BaseContextMenu.Root>
  );
}
