/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import { useParams } from "react-router";
// pi dash imports
import {
  GROUPED_PROFILE_SETTINGS,
  PROFILE_SETTINGS_CATEGORIES,
  PROFILE_SETTINGS_CATEGORY_I18N_LABELS,
} from "@pi-dash/constants";
import { useTranslation } from "@pi-dash/i18n";
// local imports
import { SettingsSidebarItem } from "../../sidebar/item";
import { ProfileSettingsSidebarWorkspaceOptions } from "./workspace-options";

export const ProfileSettingsSidebarItemCategories = observer(function ProfileSettingsSidebarItemCategories() {
  // params — the active tab is the route, not component state
  const { workspaceSlug, profileTabId } = useParams();
  // translation
  const { t } = useTranslation();

  return (
    <div className="mt-4 flex flex-col gap-y-4">
      {PROFILE_SETTINGS_CATEGORIES.map((category) => {
        const categoryItems = GROUPED_PROFILE_SETTINGS[category];

        if (categoryItems.length === 0) return null;

        return (
          <div key={category} className="shrink-0">
            <div className="p-2 text-caption-md-medium text-tertiary capitalize">
              {t(PROFILE_SETTINGS_CATEGORY_I18N_LABELS[category])}
            </div>
            <div className="flex flex-col">
              {categoryItems.map((item) => (
                <SettingsSidebarItem
                  key={item.key}
                  as="link"
                  href={`/${workspaceSlug}/settings/account/${item.key}`}
                  isActive={profileTabId === item.key}
                  icon={item.icon}
                  label={t(item.i18n_label)}
                />
              ))}
            </div>
          </div>
        );
      })}
      <ProfileSettingsSidebarWorkspaceOptions />
    </div>
  );
});
