/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { NavLink, Outlet } from "react-router";
import { useParams } from "react-router";

export default function RunnersLayout() {
  const { workspaceSlug } = useParams<{ workspaceSlug: string }>();
  const base = `/${workspaceSlug}/runners`;
  const tabs = [
    { to: base, label: "Runners", end: true },
    { to: `${base}/runs`, label: "Runs" },
    { to: `${base}/approvals`, label: "Approvals" },
  ];
  return (
    <div className="flex h-full w-full flex-col">
      <div className="flex items-center gap-4 border-b px-6 py-3">
        <h1 className="text-lg font-semibold">Apple Pi Dash Runner</h1>
        <nav className="flex gap-2">
          {tabs.map((t) => (
            <NavLink
              key={t.to}
              to={t.to}
              end={t.end}
              className={({ isActive }) =>
                `text-sm rounded px-3 py-1 ${isActive ? "bg-neutral-200 font-medium" : "hover:bg-neutral-100"}`
              }
            >
              {t.label}
            </NavLink>
          ))}
        </nav>
      </div>
      <div className="flex-1 overflow-auto p-6">
        <Outlet />
      </div>
    </div>
  );
}
