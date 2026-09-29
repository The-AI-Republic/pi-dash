/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import { usePathname } from "next/navigation";
// pi dash imports
import type { TUserPermissions, TUserPermissionsLevel } from "@pi-dash/constants";
import {
  EUserPermissionsLevel,
  GROUPED_WORKSPACE_SETTINGS,
  WORKSPACE_SETTINGS_CATEGORIES,
  WORKSPACE_SETTINGS_CATEGORY_I18N_LABELS,
} from "@pi-dash/constants";
import { useTranslation } from "@pi-dash/i18n";
import type { EUserWorkspaceRoles, TWorkspaceSettingsItem } from "@pi-dash/types";
import { cn, joinUrlPath } from "@pi-dash/utils";
// components
import { SettingsSidebarItem } from "@/components/settings/sidebar/item";
// hooks
import { useUserPermissions } from "@/hooks/store/user";
// local imports
import { WORKSPACE_SETTINGS_ICONS } from "../workspace/sidebar/item-icon";

export type TWorkspaceSettingsCategoryGroup = {
  category: (typeof WORKSPACE_SETTINGS_CATEGORIES)[number];
  items: TWorkspaceSettingsItem[];
};

type TAllowPermissions = (
  allowPermissions: (TUserPermissions | EUserWorkspaceRoles)[],
  level: TUserPermissionsLevel,
  workspaceSlug?: string
) => boolean;

/**
 * The workspace settings items the signed-in user may open in `workspaceSlug`, grouped by
 * category, with inaccessible items and then empty categories dropped.
 *
 * The slug is a parameter rather than something read from the route on purpose: the account
 * settings navigator lists *every* workspace the user belongs to, so each row has to be
 * filtered against its own workspace instead of the current one.
 */
export const getAccessibleWorkspaceSettingGroups = (
  workspaceSlug: string,
  allowPermissions: TAllowPermissions
): TWorkspaceSettingsCategoryGroup[] =>
  WORKSPACE_SETTINGS_CATEGORIES.map((category) => ({
    category,
    items: GROUPED_WORKSPACE_SETTINGS[category].filter((item) =>
      allowPermissions(item.access, EUserPermissionsLevel.WORKSPACE, workspaceSlug)
    ),
  })).filter((group) => group.items.length > 0);

/**
 * `general` lives at the settings root, so a prefix test would light it up on every other
 * tab as well - it has to match exactly. Everything else matches on prefix so nested pages
 * keep their parent highlighted. Both are scoped to `workspaceSlug`, so only the workspace
 * actually being viewed highlights.
 */
export const isWorkspaceSettingItemActive = (
  pathname: string,
  workspaceSlug: string,
  item: TWorkspaceSettingsItem
): boolean =>
  item.href === "/settings"
    ? pathname === `/${workspaceSlug}${item.href}/`
    : new RegExp(`^/${workspaceSlug}${item.href}/`).test(pathname);

export const useAccessibleWorkspaceSettings = (workspaceSlug: string): TWorkspaceSettingsCategoryGroup[] => {
  const { allowPermissions } = useUserPermissions();
  return getAccessibleWorkspaceSettingGroups(workspaceSlug, allowPermissions);
};

type Props = {
  workspaceSlug: string;
  className?: string;
  categoryClassName?: string;
};

/**
 * The workspace settings item groups, shared by the standalone `/:workspaceSlug/settings*`
 * navigator and the per-workspace groups nested in the account settings navigator, so the
 * two lists cannot drift.
 */
export const SettingsSidebarWorkspaceSettingGroups = observer(function SettingsSidebarWorkspaceSettingGroups(
  props: Props
) {
  const { workspaceSlug, className, categoryClassName } = props;
  // next hooks
  const pathname = usePathname();
  // derived values
  const groups = useAccessibleWorkspaceSettings(workspaceSlug);
  // translation
  const { t } = useTranslation();

  if (groups.length === 0) return null;

  return (
    <div className={className}>
      {groups.map(({ category, items }) => (
        <div key={category} className={cn("shrink-0", categoryClassName)}>
          <div className="p-2 text-caption-md-medium text-tertiary capitalize">
            {t(WORKSPACE_SETTINGS_CATEGORY_I18N_LABELS[category])}
          </div>
          <div className="flex flex-col">
            {items.map((item) => {
              const label =
                item.key === "integrations" ? (
                  <>
                    {t(item.i18n_label)} <span className="text-caption-sm-medium text-tertiary">(github)</span>
                  </>
                ) : (
                  t(item.i18n_label)
                );

              return (
                <SettingsSidebarItem
                  key={item.key}
                  as="link"
                  href={joinUrlPath(workspaceSlug, item.href)}
                  isActive={isWorkspaceSettingItemActive(pathname, workspaceSlug, item)}
                  icon={WORKSPACE_SETTINGS_ICONS[item.key]}
                  label={label}
                />
              );
            })}
          </div>
        </div>
      ))}
    </div>
  );
});
