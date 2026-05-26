/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect } from "react";
import { observer } from "mobx-react";
// pi dash imports
import { PROFILE_SETTINGS_TABS } from "@pi-dash/constants";
import { useTranslation } from "@pi-dash/i18n";
import type { TProfileSettingsTabs } from "@pi-dash/types";
// components
import { LogoSpinner } from "@/components/common/logo-spinner";
import { PageHead } from "@/components/core/page-title";
import { ProfileSettingsContent } from "@/components/settings/profile/content";
import { ProfileSettingsSidebarRoot } from "@/components/settings/profile/sidebar";
// hooks
import { useInstance } from "@/hooks/store/use-instance";
import { useUser } from "@/hooks/store/user";
import { useAppRouter } from "@/hooks/use-app-router";
// local imports
import type { Route } from "../+types/layout";

function ProfileSettingsPage(props: Route.ComponentProps) {
  const { profileTabId } = props.params;
  // router
  const router = useAppRouter();
  // store hooks
  const { data: currentUser } = useUser();
  const { config } = useInstance();
  // translation
  const { t } = useTranslation();
  // derived values
  const isAValidTab = PROFILE_SETTINGS_TABS.includes(profileTabId as TProfileSettingsTabs);
  // Password management is owned by the upstream IdP on hosted deployments;
  // the security tab is hidden in the sidebar there, but a direct URL nav
  // would still load the change-password form, so we redirect away.
  const isSecurityTabBlocked = profileTabId === "security" && config?.is_self_managed === false;

  useEffect(() => {
    if (isSecurityTabBlocked) router.replace("/settings/profile/general");
  }, [isSecurityTabBlocked, router]);

  if (!currentUser || !isAValidTab || isSecurityTabBlocked)
    return (
      <div className="grid size-full place-items-center px-4">
        <LogoSpinner />
      </div>
    );

  return (
    <>
      <PageHead title={`${t("profile.label")} - ${t("general_settings")}`} />
      <div className="relative size-full">
        <div className="flex size-full">
          <ProfileSettingsSidebarRoot
            activeTab={profileTabId as TProfileSettingsTabs}
            className="w-[250px]"
            updateActiveTab={(tab) => router.push(`/settings/profile/${tab}`)}
          />
          <ProfileSettingsContent
            activeTab={profileTabId as TProfileSettingsTabs}
            className="mx-auto w-fit max-w-225 grow px-page-x py-20"
          />
        </div>
      </div>
    </>
  );
}

export default observer(ProfileSettingsPage);
