/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { ReactNode } from "react";
import type { IWorkspaceSidebarNavigationItem } from "@pi-dash/constants";
import { SidebarItemBase } from "@/components/workspace/sidebar/sidebar-item";

type Props = {
  item: IWorkspaceSidebarNavigationItem;
  additionalStaticItems?: string[];
  /** Overrides the key-based icon lookup; see `SidebarItemBase`. */
  icon?: ReactNode;
};

export function SidebarItem({ item, additionalStaticItems, icon }: Props) {
  return <SidebarItemBase item={item} additionalStaticItems={additionalStaticItems} icon={icon} />;
}
