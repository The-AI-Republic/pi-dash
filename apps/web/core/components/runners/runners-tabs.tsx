/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { NavLink, useParams } from "react-router";
import { useTranslation } from "@pi-dash/i18n";

export function RunnersTabs() {
  const { workspaceSlug, projectId } = useParams<{ workspaceSlug: string; projectId?: string }>();
  const { t } = useTranslation();
  // Project-scoped panel and the workspace-wide aggregate share these tabs; the
  // only difference is the link base, derived from whether a projectId is in
  // the route.
  const base = projectId ? `/${workspaceSlug}/projects/${projectId}/runners` : `/${workspaceSlug}/runners`;

  const tabs: { to: string; label: string; end: boolean }[] = [
    { to: base, label: t("Overview"), end: true },
    { to: `${base}/runs`, label: t("Runs"), end: false },
    { to: `${base}/approvals`, label: t("Approvals"), end: false },
  ];

  return (
    <div className="flex border-b border-subtle">
      {tabs.map((tab) => (
        <NavLink
          key={tab.to}
          to={tab.to}
          end={tab.end}
          className={({ isActive }) =>
            `flex h-9 items-center border-b-2 px-3 text-13 font-medium transition-colors ${
              isActive ? "border-accent-strong text-primary" : "border-transparent text-secondary hover:text-primary"
            }`
          }
        >
          {tab.label}
        </NavLink>
      ))}
    </div>
  );
}
