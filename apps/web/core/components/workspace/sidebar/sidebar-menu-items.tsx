/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import React, { useMemo } from "react";
import { observer } from "mobx-react";
// pi dash imports
import {
  WORKSPACE_SIDEBAR_STATIC_NAVIGATION_ITEMS_LINKS,
  WORKSPACE_SIDEBAR_STATIC_PINNED_NAVIGATION_ITEMS_LINKS,
} from "@pi-dash/constants";
// constants
import { extendedNavigationItems } from "@/constants/extended-navigation";
// pi-dash-web imports
import { SidebarItem } from "@/pi-dash-web/components/workspace/sidebar/sidebar-item";

// Items relocated out of this section by the AI-orchestration layout:
//   - drafts → top of Projects section
//   - views → "Work Items" row in Projects section
//   - projects, analytics, prompts, schedulers, archives → new "More" section
//   - runners → "More" section (the workspace-wide AI Workers aggregate; the
//     per-project AI Workers panel now lives in the project sidebar)
// All entries from WORKSPACE_SIDEBAR_DYNAMIC_NAVIGATION_ITEMS_LINKS are relocated,
// so that list is no longer rendered here.
const RELOCATED_KEYS = new Set([
  "drafts",
  "views",
  "projects",
  "analytics",
  "prompts",
  "schedulers",
  "archives",
  "runners",
]);

// Keys contributed by the running build, treated as always-visible for the
// same reason the built-in rows are: they are navigation, not user-pinnable
// favourites.
const extendedNavigationKeys = extendedNavigationItems.map((item) => item.key);

export const SidebarMenuItems = observer(function SidebarMenuItems() {
  // "Your work" used to be appended here as a user-pinnable personal item; it
  // now lives in the user menu popup (see user-menu-root.tsx), so this section
  // renders only the always-visible built-in rows.
  const filteredStaticNavigationItems = useMemo(
    () => WORKSPACE_SIDEBAR_STATIC_NAVIGATION_ITEMS_LINKS.filter((item) => !RELOCATED_KEYS.has(item.key)),
    []
  );

  // Workspace-pinned items (all relocated to "More" now, including `runners`;
  // computed via the RELOCATED_KEYS filter so the set of survivors stays in
  // lockstep with that single source of truth).
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
      {/* Contributed by the running build; empty in open source.
          `additionalStaticItems` is required, not decorative: SidebarItem
          hides anything that is neither pinned by the user nor in its own
          hardcoded static list, and that list cannot know about keys a
          downstream build introduces. */}
      {extendedNavigationItems.map((item) => (
        <SidebarItem
          key={`extended_${item.key}`}
          item={item}
          icon={item.icon}
          additionalStaticItems={extendedNavigationKeys}
        />
      ))}
    </div>
  );
});
