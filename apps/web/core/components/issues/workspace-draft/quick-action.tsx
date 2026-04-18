/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import { useTranslation } from "@apple-pi-dash/i18n";
// ui
import type { TContextMenuItem } from "@apple-pi-dash/ui";
import { ContextMenu, CustomMenu } from "@apple-pi-dash/ui";
// helpers
import { cn } from "@apple-pi-dash/utils";

export interface Props {
  parentRef: React.RefObject<HTMLElement>;
  MENU_ITEMS: TContextMenuItem[];
}

export const WorkspaceDraftIssueQuickActions = observer(function WorkspaceDraftIssueQuickActions(props: Props) {
  const { parentRef, MENU_ITEMS } = props;

  const { t } = useTranslation();

  return (
    <>
      <ContextMenu parentRef={parentRef} items={MENU_ITEMS} />
      <CustomMenu
        ellipsis
        placement="bottom-end"
        menuItemsClassName="z-[14]"
        maxHeight="lg"
        useCaptureForOutsideClick
        closeOnSelect
      >
        {MENU_ITEMS.map((item) => (
          <CustomMenu.MenuItem
            key={item.key}
            onClick={() => {
              item.action();
            }}
            className={cn(
              "flex items-center gap-2",
              {
                "text-placeholder": item.disabled,
              },
              item.className
            )}
            disabled={item.disabled}
          >
            {item.icon && <item.icon className={cn("h-3 w-3", item.iconClassName)} />}
            <div>
              <h5>{t(item.title || "")}</h5>
              {item.description && (
                <p
                  className={cn("whitespace-pre-line text-tertiary", {
                    "text-placeholder": item.disabled,
                  })}
                >
                  {item.description}
                </p>
              )}
            </div>
          </CustomMenu.MenuItem>
        ))}
      </CustomMenu>
    </>
  );
});
