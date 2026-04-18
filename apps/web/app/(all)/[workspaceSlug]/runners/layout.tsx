/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import { NavLink, Outlet, useParams } from "react-router";
import { EUserPermissions, EUserPermissionsLevel } from "@apple-pi-dash/constants";
import { useTranslation } from "@apple-pi-dash/i18n";
import { NotAuthorizedView } from "@/components/auth-screens/not-authorized-view";
import { useUserPermissions } from "@/hooks/store/user";

const RunnersLayout = observer(function RunnersLayout() {
  const { workspaceSlug } = useParams<{ workspaceSlug: string }>();
  const { workspaceUserInfo, allowPermissions } = useUserPermissions();
  const { t } = useTranslation();

  const canViewRunners = allowPermissions(
    [EUserPermissions.ADMIN, EUserPermissions.MEMBER],
    EUserPermissionsLevel.WORKSPACE
  );

  if (workspaceUserInfo && !canViewRunners) {
    return <NotAuthorizedView section="general" className="h-auto" />;
  }

  const base = `/${workspaceSlug}/runners`;
  const tabs = [
    { to: base, label: t("runners.tabs.runners"), end: true },
    { to: `${base}/runs`, label: t("runners.tabs.runs"), end: false },
    { to: `${base}/approvals`, label: t("runners.tabs.approvals"), end: false },
  ];

  return (
    <div className="flex h-full w-full flex-col">
      <div className="flex items-center gap-4 border-b border-subtle px-6 py-3">
        <h1 className="text-16 font-semibold text-primary">{t("runners.title")}</h1>
        <nav className="flex gap-2">
          {tabs.map((tab) => (
            <NavLink
              key={tab.to}
              to={tab.to}
              end={tab.end}
              className={({ isActive }) =>
                `rounded px-3 py-1 text-13 ${isActive ? "bg-layer-1 font-medium text-primary" : "text-secondary hover:bg-layer-1"}`
              }
            >
              {tab.label}
            </NavLink>
          ))}
        </nav>
      </div>
      <div className="flex-1 overflow-auto p-6">
        <Outlet />
      </div>
    </div>
  );
});

export default RunnersLayout;
