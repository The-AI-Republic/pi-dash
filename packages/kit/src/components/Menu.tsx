// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Menu basics (UI kit page): one menu-button popup shared by every
// dropdown. Actions may carry a secondary description line (a disabled
// entry explains itself there), and a popup may hold caller-rendered
// custom rows between its actions.
import { Menu as BaseMenu } from "@base-ui-components/react/menu";
import * as React from "react";
import { cn } from "../lib/cn";
import { pickDefined } from "../lib/props";

export interface MenuAction {
  label: string;
  onSelect: () => void;
  disabled?: boolean;
  /**
   * Secondary line under the label: why an entry is disabled, or what it
   * does. The item's accessible name covers the label and the note.
   */
  description?: string;
  /** Leading mark (an avatar or glyph); hidden from assistive tech. */
  icon?: React.ReactNode;
  /** React key when sibling labels repeat (project names can collide). */
  itemKey?: string;
  /**
   * Leave the menu open after a select. Multi-select menus (a filter row
   * that toggles a checkmark) set false so several rows can be changed in
   * one visit; the default closes on select.
   */
  closeOnClick?: boolean;
  /**
   * Rich row body replacing the plain label (the workspace switcher
   * renders logo, role and member counts here). The item keeps its
   * menuitem role and arrow-key behavior; the accessible name derives
   * from the rendered content.
   */
  content?: React.ReactNode;
}

export interface MenuSeparator {
  separator: true;
}

/**
 * Caller-rendered popup content: identity rows, embedded forms, or any
 * node that must sit in the menu without a menuitem wrapper. The node
 * renders as a direct popup child; menu semantics inside it (a nested
 * menuitem, a form) are the caller's own.
 */
export interface MenuCustom {
  custom: true;
  /** React key: custom entries carry no label to key on. */
  entryKey: string;
  node: React.ReactNode;
}

export type MenuEntry = MenuAction | MenuSeparator | MenuCustom;

export function isMenuSeparator(entry: MenuEntry): entry is MenuSeparator {
  return "separator" in entry;
}

export function isMenuCustom(entry: MenuEntry): entry is MenuCustom {
  return "custom" in entry;
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

/**
 * The text block inside a menu item: the label, or the caller's rich
 * content, followed by the optional secondary note. ContextMenu shares it
 * so both popups read the same.
 */
export function MenuItemBody({
  label,
  description,
  content,
}: {
  label: string;
  description?: string | undefined;
  content?: React.ReactNode;
}): React.ReactElement {
  if (description === undefined) return <>{content ?? label}</>;
  return (
    <span className="flex min-w-0 flex-col">
      <span data-menu-title="true" className="min-w-0">
        {content ?? label}
      </span>
      <span data-menu-description="true" className="text-caption text-(--text-muted)">
        {description}
      </span>
    </span>
  );
}

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
              ) : isMenuCustom(entry) ? (
                <React.Fragment key={entry.entryKey}>{entry.node}</React.Fragment>
              ) : (
                <BaseMenu.Item
                  key={entry.itemKey ?? entry.label}
                  disabled={entry.disabled ?? false}
                  closeOnClick={entry.closeOnClick ?? true}
                  onClick={entry.onSelect}
                  className={cn(itemClasses)}
                >
                  {entry.icon === undefined ? null : (
                    <span aria-hidden="true" className="mr-(--space-2) inline-flex shrink-0 items-center">
                      {entry.icon}
                    </span>
                  )}
                  <MenuItemBody label={entry.label} description={entry.description} content={entry.content} />
                </BaseMenu.Item>
              )
            )}
          </BaseMenu.Popup>
        </BaseMenu.Positioner>
      </BaseMenu.Portal>
    </BaseMenu.Root>
  );
}
