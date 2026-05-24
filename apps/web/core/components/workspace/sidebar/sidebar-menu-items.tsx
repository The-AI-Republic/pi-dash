/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import React, { useMemo } from "react";
import { orderBy } from "lodash-es";
import { observer } from "mobx-react";
// pi dash imports
import {
  WORKSPACE_SIDEBAR_DYNAMIC_NAVIGATION_ITEMS_LINKS,
  WORKSPACE_SIDEBAR_STATIC_NAVIGATION_ITEMS,
  WORKSPACE_SIDEBAR_STATIC_NAVIGATION_ITEMS_LINKS,
  WORKSPACE_SIDEBAR_STATIC_PINNED_NAVIGATION_ITEMS_LINKS,
} from "@pi-dash/constants";
// store hooks
import {
  usePersonalNavigationPreferences,
  useWorkspaceNavigationPreferences,
} from "@/hooks/use-navigation-preferences";
// pi-dash-web imports
import { SidebarItem } from "@/pi-dash-web/components/workspace/sidebar/sidebar-item";

// Items relocated out of this section by the AI-orchestration layout:
//   - drafts → top of Projects section
//   - views → "Work Items" row in Projects section
//   - analytics, prompts, schedulers, archives → new "More" section
const RELOCATED_KEYS = new Set(["drafts", "views", "analytics", "prompts", "schedulers", "archives"]);

export const SidebarMenuItems = observer(function SidebarMenuItems() {
  // hooks
  const { preferences: personalPreferences } = usePersonalNavigationPreferences();
  const { preferences: workspacePreferences } = useWorkspaceNavigationPreferences();

  // Personal items (Stickies / Your work) gated by user preferences, sorted.
  const filteredStaticNavigationItems = useMemo(() => {
    const items = [...WORKSPACE_SIDEBAR_STATIC_NAVIGATION_ITEMS_LINKS];
    const personalItems: Array<(typeof items)[0] & { sort_order: number }> = [];

    const stickiesItem = WORKSPACE_SIDEBAR_STATIC_NAVIGATION_ITEMS["stickies"];
    if (personalPreferences.items.stickies?.enabled && stickiesItem) {
      personalItems.push({
        ...stickiesItem,
        sort_order: personalPreferences.items.stickies.sort_order,
      });
    }
    if (personalPreferences.items.your_work?.enabled && WORKSPACE_SIDEBAR_STATIC_NAVIGATION_ITEMS["your-work"]) {
      personalItems.push({
        ...WORKSPACE_SIDEBAR_STATIC_NAVIGATION_ITEMS["your-work"],
        sort_order: personalPreferences.items.your_work.sort_order,
      });
    }
    personalItems.sort((a, b) => a.sort_order - b.sort_order);

    return [...items, ...personalItems].filter((item) => !RELOCATED_KEYS.has(item.key));
  }, [personalPreferences]);

  // Workspace-pinned items (projects, runners — prompts/schedulers are relocated).
  const pinnedNavigationItems = useMemo(
    () => WORKSPACE_SIDEBAR_STATIC_PINNED_NAVIGATION_ITEMS_LINKS.filter((item) => !RELOCATED_KEYS.has(item.key)),
    []
  );

  // User-pinned dynamic items, sorted (views/analytics/archives are relocated).
  const sortedNavigationItems = useMemo(
    () =>
      orderBy(
        WORKSPACE_SIDEBAR_DYNAMIC_NAVIGATION_ITEMS_LINKS.filter((item) => !RELOCATED_KEYS.has(item.key)),
        [(item) => workspacePreferences.items[item.key]?.sort_order ?? 0],
        ["asc"]
      ),
    [workspacePreferences]
  );

  return (
    <div className="flex flex-col gap-0.5">
      {filteredStaticNavigationItems.map((item) => (
        <SidebarItem key={`static_${item.key}`} item={item} />
      ))}
      {pinnedNavigationItems.map((item) => (
        <SidebarItem key={`pinned_${item.key}`} item={item} />
      ))}
      {sortedNavigationItems.map((item) => (
        <SidebarItem key={`dynamic_${item.key}`} item={item} />
      ))}
    </div>
  );
});
