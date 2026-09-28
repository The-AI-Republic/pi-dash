/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import { usePathname } from "next/navigation";
import { Outlet } from "react-router";
// components
import { getAccountActivePath } from "@/components/settings/helper";
import { SettingsMobileNav } from "@/components/settings/mobile/nav";
import { ProfileSettingsSidebarRoot } from "@/components/settings/profile/sidebar";

// Account (profile) settings share the workspace settings shell: the navigator
// is the middle panel of the app shell and each tab renders through `<Outlet />`,
// so every tab is a real URL instead of a piece of component state.
const AccountSettingsLayout = observer(function AccountSettingsLayout() {
  // next hooks
  const pathname = usePathname();

  return (
    <>
      <SettingsMobileNav
        hamburgerContent={ProfileSettingsSidebarRoot}
        activePath={getAccountActivePath(pathname) || ""}
      />
      <div className="relative flex size-full">
        <div className="hidden h-full md:block">
          <ProfileSettingsSidebarRoot />
        </div>
        <Outlet />
      </div>
    </>
  );
});

export default AccountSettingsLayout;
