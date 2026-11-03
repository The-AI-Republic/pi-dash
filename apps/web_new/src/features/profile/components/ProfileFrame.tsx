// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Profile page frame (parity/specs/profile.md §1 and §4, PROF-001/
// PROF-002/PROF-008). Owns the breadcrumb, the role's tab strip or narrow
// picker, the identity sidebar and the content column; every tab renders
// through the outlet inside it, so an unknown or denied tab keeps the
// frame. The sidebar is hidden for signed-out-less states; the route only
// mounts this frame once the segregation read resolved.

import { Breadcrumbs } from "@pidash/kit";
import { Outlet, useRouterState } from "@tanstack/react-router";
import * as React from "react";

import type { UserProfileSegregation } from "@pidash/api-client";
import type { Permissions } from "../../../core/session/permissions.js";
import { useNarrowViewport } from "../../../shared/shell/narrowViewport.js";
import { profileBreadcrumbLabel } from "../lib/identity.js";
import { profileBase } from "../lib/tabs.js";
import { ProfileMobileTabPicker } from "./ProfileMobileTabPicker.js";
import { ProfileSidebarFrame, type ProfileProjectName } from "./ProfileSidebar.js";
import { ProfileTabStrip } from "./ProfileTabStrip.js";

export interface ProfileFrameProps {
  workspaceSlug: string;
  userId: string;
  segregation: UserProfileSegregation;
  permissions: Permissions;
  isOwn: boolean;
  /** Workspace project names for the sidebar rows. */
  projects: readonly ProfileProjectName[];
  apiBaseUrl: string;
}

export function ProfileFrame({
  workspaceSlug,
  userId,
  segregation,
  permissions,
  isOwn,
  projects,
  apiBaseUrl,
}: ProfileFrameProps): React.ReactElement {
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  const narrow = useNarrowViewport();
  return (
    <div data-testid="profile-page" className="flex h-full min-h-0 flex-col">
      <header className="flex shrink-0 flex-col gap-(--space-1) border-b border-(--border) px-(--space-4) py-(--space-2)">
        <Breadcrumbs
          label="Profile breadcrumb"
          entries={[
            { label: "Profile", href: profileBase(workspaceSlug, userId) },
            { label: profileBreadcrumbLabel(segregation.user_data, isOwn) },
          ]}
        />
      </header>
      {narrow ? (
        <ProfileMobileTabPicker
          workspaceSlug={workspaceSlug}
          userId={userId}
          permissions={permissions}
          pathname={pathname}
        />
      ) : (
        <ProfileTabStrip workspaceSlug={workspaceSlug} userId={userId} permissions={permissions} pathname={pathname} />
      )}
      <div className="flex min-h-0 flex-1">
        <ProfileSidebarFrame
          segregation={segregation}
          workspaceSlug={workspaceSlug}
          isOwn={isOwn}
          projects={projects}
          apiBaseUrl={apiBaseUrl}
          narrow={narrow}
        />
        <main className="flex min-h-0 min-w-0 flex-1 flex-col">
          <Outlet />
        </main>
      </div>
    </div>
  );
}
