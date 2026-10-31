// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar account menu (ADM-064, ADM-065, from parity/specs/admin.md
// §8.3/§8.4): the admin label with avatar trigger opening the menu
// with the account email, the theme switch, and the native sign-out
// form. Collapsed, the label hides and the same menu hangs off the
// icon trigger. The shell mounts only after the session resolves, so
// the trigger always renders loaded; there is no skeleton variant.
import { Moon, Sun } from "lucide-react";
import * as React from "react";

import { Avatar, Menu, resolveTheme } from "@pidash/kit";

import { useAdminSession } from "../hooks.js";
import { useAdminSidebarStore } from "../stores.js";
import { useAdminThemeStore } from "../theme.js";
import { AdminSignOutForm } from "./AdminSignOutForm.js";

export function AdminAccountMenu(): React.ReactElement {
  const collapsed = useAdminSidebarStore((state) => state.collapsed);
  const session = useAdminSession();
  const admin = session.data?.kind === "admin" ? session.data.admin : null;
  const email = admin?.email ?? "";
  const displayName = admin?.display_name || email || "Instance admin";
  const avatarSrc = admin?.avatar_url ?? undefined;
  const mode = useAdminThemeStore((state) => state.mode);
  const setMode = useAdminThemeStore((state) => state.setMode);
  const target = resolveTheme(mode) === "dark" ? "light" : "dark";
  return (
    <div data-testid="admin-account" className="flex items-center gap-(--space-2) p-(--space-3)">
      {collapsed ? null : (
        <p data-testid="admin-account-label" className="text-body min-w-0 flex-1 truncate font-medium text-(--text)">
          Instance admin
        </p>
      )}
      <Menu
        trigger={
          <button
            type="button"
            aria-label="Admin account"
            data-testid="admin-account-trigger"
            className="cursor-pointer rounded-(--radius-full)"
          >
            {avatarSrc === undefined ? (
              <Avatar name={displayName} size="small" />
            ) : (
              <Avatar name={displayName} src={avatarSrc} size="small" />
            )}
          </button>
        }
        items={[
          {
            custom: true,
            entryKey: "identity",
            node: (
              <span
                data-testid="admin-account-email"
                className="text-body block truncate px-(--space-4) py-(--space-2) text-(--text-muted)"
              >
                {email}
              </span>
            ),
          },
          {
            label: `Switch to ${target} mode`,
            icon: target === "dark" ? <Moon size={16} aria-hidden="true" /> : <Sun size={16} aria-hidden="true" />,
            onSelect: () => setMode(target),
          },
          { custom: true, entryKey: "sign-out", node: <AdminSignOutForm /> },
        ]}
      />
    </div>
  );
}
