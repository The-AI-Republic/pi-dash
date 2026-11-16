// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The whole-list ellipsis (parity/specs/issues.md §7, ISS-067). It sits
// in a list page's header and offers the page's own address — open it in
// another tab, or copy it — then any entries the edition registered
// through the seam. The menu reads the address from the router at click
// time so a filtered view copies the filters it is showing.

import { IconButton, Menu, type MenuEntry } from "@pidash/kit";
import { Ellipsis, Link2, ArrowUpRight } from "lucide-react";
import * as React from "react";

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { copyLinkWithToast } from "../../../shared/shell/shareLink.js";
import { listMenuExtraEntries } from "./listMenuExtension.js";
import { COPY_LINK_COPIED_TITLE, LIST_COPY_FAILED_TITLE, LIST_COPY_MESSAGE, LIST_MENU_LABEL } from "./messages.js";
import { useCurrentPageHref } from "./useCurrentPageHref.js";

/** "path?search" for the page the menu lives on. */
export function currentPagePath(pathname: string, search: string): string {
  return search === "" ? pathname : `${pathname}${search.startsWith("?") ? search : `?${search}`}`;
}

export function ListPageQuickActions(): React.ReactElement {
  const href = useCurrentPageHref();
  const extras = listMenuExtraEntries();

  const items: MenuEntry[] = [
    {
      label: "Open in new tab",
      itemKey: "openInNewTab",
      icon: <ArrowUpRight size={14} aria-hidden="true" />,
      onSelect: () => globalThis.window.open(href, "_blank", "noopener,noreferrer"),
    },
    {
      label: "Copy link",
      itemKey: "copyLink",
      icon: <Link2 size={14} aria-hidden="true" />,
      onSelect: () =>
        void copyLinkWithToast(
          href,
          { copied: COPY_LINK_COPIED_TITLE, copiedBody: LIST_COPY_MESSAGE, failed: LIST_COPY_FAILED_TITLE },
          { success: notifySuccess, error: notifyError }
        ),
    },
    ...extras.map((extra) => ({
      label: extra.label,
      itemKey: extra.key,
      onSelect: extra.onSelect,
    })),
  ];

  return (
    <Menu
      trigger={
        <IconButton label={LIST_MENU_LABEL} size="small" data-testid="list-page-actions">
          <Ellipsis size={16} strokeWidth={1.5} aria-hidden="true" />
        </IconButton>
      }
      items={items}
    />
  );
}
