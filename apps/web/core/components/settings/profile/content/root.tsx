/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { Suspense } from "react";
import { observer } from "mobx-react";
// apple pi dash imports
import { ScrollArea } from "@apple-pi-dash/propel/scrollarea";
import type { TProfileSettingsTabs } from "@apple-pi-dash/types";
import { cn } from "@apple-pi-dash/utils";
// local imports
import { PROFILE_SETTINGS_PAGES_MAP } from "./pages";

type Props = {
  activeTab: TProfileSettingsTabs;
  className?: string;
};

export const ProfileSettingsContent = observer(function ProfileSettingsContent(props: Props) {
  const { activeTab, className } = props;
  const PageComponent = PROFILE_SETTINGS_PAGES_MAP[activeTab];

  return (
    <ScrollArea
      className={cn("shrink-0 overflow-y-scroll bg-surface-1", className)}
      viewportClassName="px-8 py-9"
      scrollType="hover"
      orientation="vertical"
      size="sm"
    >
      <Suspense>
        <PageComponent />
      </Suspense>
    </ScrollArea>
  );
});
