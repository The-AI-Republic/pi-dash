// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { Menu as BaseMenu } from "@base-ui-components/react/menu";
import * as React from "react";
import { cn } from "../lib/cn";
import { pickDefined } from "../lib/props";

export interface MenuAction {
  label: string;
  onSelect: () => void;
  disabled?: boolean;
}

export interface MenuSeparator {
  separator: true;
}

export type MenuEntry = MenuAction | MenuSeparator;

export function isMenuSeparator(entry: MenuEntry): entry is MenuSeparator {
  return "separator" in entry;
}

export interface MenuProps {
  /**
   * Element rendered as the menu trigger (usually a Button). It must carry
   * an accessible name: the open menu takes its own name from the trigger,
   * following the menu-button pattern.
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

export function Menu({ trigger, items, open, onOpenChange }: MenuProps): React.ReactElement {
  return (
    <BaseMenu.Root {...pickDefined({ open, onOpenChange })}>
      <BaseMenu.Trigger render={trigger} />
      <BaseMenu.Portal>
        <BaseMenu.Positioner sideOffset={4}>
          <BaseMenu.Popup className={popupClasses}>
            {items.map((entry, index) =>
              isMenuSeparator(entry) ? (
                <div
                  key={`separator-${index}`}
                  role="separator"
                  className="mx-(--space-2) my-(--space-2) border-t border-(--border)"
                />
              ) : (
                <BaseMenu.Item
                  key={entry.label}
                  disabled={entry.disabled ?? false}
                  onClick={entry.onSelect}
                  className={cn(itemClasses)}
                >
                  {entry.label}
                </BaseMenu.Item>
              )
            )}
          </BaseMenu.Popup>
        </BaseMenu.Positioner>
      </BaseMenu.Portal>
    </BaseMenu.Root>
  );
}
