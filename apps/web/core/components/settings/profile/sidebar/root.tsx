/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

// pi dash imports
import { ScrollArea } from "@pi-dash/propel/scrollarea";
import type { TProfileSettingsTabs } from "@pi-dash/types";
import { cn } from "@pi-dash/utils";
// local imports
import { ProfileSettingsSidebarHeader } from "./header";
import { ProfileSettingsSidebarItemCategories } from "./item-categories";

type Props = {
  activeTab: TProfileSettingsTabs;
  className?: string;
  updateActiveTab: (tab: TProfileSettingsTabs) => void;
};

export function ProfileSettingsSidebarRoot(props: Props) {
  const { activeTab, className, updateActiveTab } = props;

  return (
    <ScrollArea
      scrollType="hover"
      orientation="vertical"
      size="sm"
      rootClassName={cn("shrink-0 overflow-y-scroll border-r border-r-subtle bg-surface-2 px-3 py-4", className)}
    >
      <ProfileSettingsSidebarHeader />
      <ProfileSettingsSidebarItemCategories activeTab={activeTab} updateActiveTab={updateActiveTab} />
    </ScrollArea>
  );
}
