/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import { Outlet, useParams } from "react-router";
import { EUserPermissions, EUserPermissionsLevel } from "@pi-dash/constants";
import { NotAuthorizedView } from "@/components/auth-screens/not-authorized-view";
import { useUserPermissions } from "@/hooks/store/user";

/**
 * Wrapper for ``/:workspaceSlug/ai-dev-machines``. The parent (projects)
 * layout already mounts WorkspaceShell; this layout adds the standard
 * flex-1 + overflow-auto scroll container shared by sibling routes
 * (Prompts, Schedulers) and gates view access to ADMIN/MEMBER to match
 * the sidebar entry in ``packages/constants/src/workspace.ts``.
 *
 * Loaded-state check uses ``workspaceUserInfo[slug]`` (not the bare
 * ``workspaceUserInfo`` truthiness) because the store eagerly initialises
 * the record to ``{}`` — see ``base-permissions.store.ts``. Without the
 * per-slug check, a legitimate admin briefly sees NotAuthorizedView on
 * first paint while ``fetchUserWorkspaceInfo`` resolves.
 */
const AiDevMachinesLayout = observer(function AiDevMachinesLayout() {
  const { workspaceSlug } = useParams<{ workspaceSlug: string }>();
  const { workspaceUserInfo, allowPermissions } = useUserPermissions();

  const isUserInfoLoaded = !!(workspaceSlug && workspaceUserInfo[workspaceSlug]);
  const canView = allowPermissions([EUserPermissions.ADMIN, EUserPermissions.MEMBER], EUserPermissionsLevel.WORKSPACE);

  if (!isUserInfoLoaded) {
    return null;
  }

  if (!canView) {
    return (
      <div className="flex-1 overflow-auto">
        <NotAuthorizedView section="general" className="h-auto" />
      </div>
    );
  }

  return (
    <div className="flex-1 overflow-auto">
      <Outlet />
    </div>
  );
});

export default AiDevMachinesLayout;
