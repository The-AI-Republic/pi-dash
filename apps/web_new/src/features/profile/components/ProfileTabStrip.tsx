// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Desktop profile tab strip (parity/specs/profile.md §1, PROF-002). Pure
// routing: one link per tab the role may see, authored with the trailing
// slash the route uses. The current tab carries aria-current so the strip
// marks it (the target of NEWFRONT-433; the live old app never marks).

import { Link } from "@tanstack/react-router";
import * as React from "react";

import type { Permissions } from "../../../core/session/permissions.js";
import { activeProfileTabKey, profileTabHref, visibleProfileTabs, type ProfileTabKey } from "../lib/tabs.js";

export interface ProfileTabStripProps {
  workspaceSlug: string;
  userId: string;
  permissions: Pick<Permissions, "canEdit">;
  /** Current pathname from the router. */
  pathname: string;
}

export function ProfileTabStrip({
  workspaceSlug,
  userId,
  permissions,
  pathname,
}: ProfileTabStripProps): React.ReactElement {
  const tabs = visibleProfileTabs(permissions);
  const active: ProfileTabKey | null = activeProfileTabKey(pathname, workspaceSlug, userId);
  return (
    <nav
      aria-label="Profile tabs"
      data-testid="profile-tabs"
      className="flex shrink-0 items-center gap-(--space-1) overflow-x-auto border-b border-(--border) px-(--space-4)"
    >
      {tabs.map((tab) => {
        const selected = tab.key === active;
        return (
          <Link
            key={tab.key}
            to={profileTabHref(workspaceSlug, userId, tab)}
            aria-current={selected ? "page" : undefined}
            data-profile-tab={tab.key}
            className={`text-body flex h-10 shrink-0 items-center border-b-2 px-(--space-3) font-medium ${
              selected
                ? "border-(--accent) text-(--text)"
                : "border-transparent text-(--text-muted) hover:text-(--text)"
            }`}
          >
            {tab.label}
          </Link>
        );
      })}
    </nav>
  );
}
