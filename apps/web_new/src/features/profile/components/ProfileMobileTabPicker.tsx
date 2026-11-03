// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Narrow-screen tab picker (parity/specs/profile.md §1, PROF-002). One
// menu replaces the strip below the breakpoint and offers exactly the
// role's tabs; the trigger labels the current tab (the target of
// NEWFRONT-433, whose trailing-slash match left the old label empty).

import { useNavigate } from "@tanstack/react-router";
import { Menu } from "@pidash/kit";
import { ChevronDown } from "lucide-react";
import * as React from "react";

import type { Permissions } from "../../../core/session/permissions.js";
import {
  activeProfileTabKey,
  PROFILE_TABS,
  profileTabHref,
  visibleProfileTabs,
  type ProfileTabKey,
} from "../lib/tabs.js";

export interface ProfileMobileTabPickerProps {
  workspaceSlug: string;
  userId: string;
  permissions: Pick<Permissions, "canEdit">;
  pathname: string;
}

/** Label of the tab a pathname selects; the summary when none matches. */
export function profilePickerLabel(pathname: string, workspaceSlug: string, userId: string): string {
  const key: ProfileTabKey | null = activeProfileTabKey(pathname, workspaceSlug, userId);
  return PROFILE_TABS.find((tab) => tab.key === key)?.label ?? "Summary";
}

export function ProfileMobileTabPicker({
  workspaceSlug,
  userId,
  permissions,
  pathname,
}: ProfileMobileTabPickerProps): React.ReactElement {
  const navigate = useNavigate();
  const tabs = visibleProfileTabs(permissions);
  const label = profilePickerLabel(pathname, workspaceSlug, userId);
  return (
    <div data-testid="profile-tab-picker" className="shrink-0 px-(--space-3) py-(--space-2)">
      <Menu
        trigger={
          <button
            type="button"
            aria-haspopup="menu"
            aria-label={`Profile tab, current ${label}`}
            data-testid="profile-tab-picker-trigger"
            className="text-body flex h-9 w-full items-center justify-between gap-(--space-2) rounded-(--radius-control) border border-(--border) bg-(--surface) px-(--space-3) text-(--text)"
          >
            <span data-testid="profile-tab-picker-label">{label}</span>
            <ChevronDown size={14} strokeWidth={1.5} aria-hidden="true" className="text-(--text-muted)" />
          </button>
        }
        items={tabs.map((tab) => ({
          id: tab.key,
          label: tab.label,
          onSelect: () => {
            void navigate({ href: profileTabHref(workspaceSlug, userId, tab) });
          },
        }))}
      />
    </div>
  );
}
