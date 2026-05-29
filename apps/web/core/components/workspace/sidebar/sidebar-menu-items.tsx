/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import React, { useMemo } from "react";
import { observer } from "mobx-react";
// pi dash imports
import {
  WORKSPACE_SIDEBAR_STATIC_NAVIGATION_ITEMS,
  WORKSPACE_SIDEBAR_STATIC_NAVIGATION_ITEMS_LINKS,
  WORKSPACE_SIDEBAR_STATIC_PINNED_NAVIGATION_ITEMS_LINKS,
} from "@pi-dash/constants";
// store hooks
import { usePersonalNavigationPreferences } from "@/hooks/use-navigation-preferences";
// pi-dash-web imports
import { SidebarItem } from "@/pi-dash-web/components/workspace/sidebar/sidebar-item";

// Items relocated out of this section by the AI-orchestration layout:
//   - drafts → top of Projects section
//   - views → "Work Items" row in Projects section
//   - projects, analytics, prompts, schedulers, archives → new "More" section
// All entries from WORKSPACE_SIDEBAR_DYNAMIC_NAVIGATION_ITEMS_LINKS are relocated,
// so that list is no longer rendered here.
const RELOCATED_KEYS = new Set(["drafts", "views", "projects", "analytics", "prompts", "schedulers", "archives"]);

export const SidebarMenuItems = observer(function SidebarMenuItems() {
  // hooks
  const { preferences: personalPreferences } = usePersonalNavigationPreferences();

  // Personal items (Your work) gated by user preferences, sorted.
  const filteredStaticNavigationItems = useMemo(() => {
    const items = [...WORKSPACE_SIDEBAR_STATIC_NAVIGATION_ITEMS_LINKS];
    const personalItems: Array<(typeof items)[0] & { sort_order: number }> = [];

    if (personalPreferences.items.your_work?.enabled && WORKSPACE_SIDEBAR_STATIC_NAVIGATION_ITEMS["your-work"]) {
      personalItems.push({
        ...WORKSPACE_SIDEBAR_STATIC_NAVIGATION_ITEMS["your-work"],
        sort_order: personalPreferences.items.your_work.sort_order,
      });
    }
    personalItems.sort((a, b) => a.sort_order - b.sort_order);

    return [...items, ...personalItems].filter((item) => !RELOCATED_KEYS.has(item.key));
  }, [personalPreferences]);

  // Workspace-pinned items (just `runners` after relocation; computed via the
  // RELOCATED_KEYS filter so the set of survivors stays in lockstep with that
  // single source of truth).
  const pinnedNavigationItems = useMemo(
    () => WORKSPACE_SIDEBAR_STATIC_PINNED_NAVIGATION_ITEMS_LINKS.filter((item) => !RELOCATED_KEYS.has(item.key)),
    []
  );

  return (
    <div className="flex flex-col gap-0.5">
      {filteredStaticNavigationItems.map((item) => (
        <SidebarItem key={`static_${item.key}`} item={item} />
      ))}
      {pinnedNavigationItems.map((item) => (
        <SidebarItem key={`pinned_${item.key}`} item={item} />
      ))}
    </div>
  );
});
