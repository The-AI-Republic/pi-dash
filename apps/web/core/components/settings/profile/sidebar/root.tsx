/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

// pi dash imports
import { ScrollArea } from "@pi-dash/propel/scrollarea";
import { cn } from "@pi-dash/utils";
// local imports
import { ProfileSettingsSidebarHeader } from "./header";
import { ProfileSettingsSidebarItemCategories } from "./item-categories";

type Props = {
  className?: string;
};

export function ProfileSettingsSidebarRoot(props: Props) {
  const { className } = props;

  return (
    <ScrollArea
      scrollType="hover"
      orientation="vertical"
      size="sm"
      rootClassName={cn(
        "h-full w-[250px] shrink-0 animate-fade-in overflow-y-scroll border-r border-r-subtle bg-surface-2 px-3 py-4",
        className
      )}
    >
      <ProfileSettingsSidebarHeader />
      <ProfileSettingsSidebarItemCategories />
    </ScrollArea>
  );
}
