/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import { Outlet } from "react-router";
import { EUserPermissions, EUserPermissionsLevel } from "@pi-dash/constants";
import { NotAuthorizedView } from "@/components/auth-screens/not-authorized-view";
import { useUserPermissions } from "@/hooks/store/user";

/**
 * Wrapper for the ``/:workspaceSlug/prompts/*`` routes. Any active workspace
 * member (Admin / Member / Guest) can *view* prompt templates; create and
 * edit operations are gated per-action on the page, not here.
 */
const PromptsLayout = observer(function PromptsLayout() {
  const { workspaceUserInfo, allowPermissions } = useUserPermissions();

  const canView = allowPermissions(
    [EUserPermissions.ADMIN, EUserPermissions.MEMBER, EUserPermissions.GUEST],
    EUserPermissionsLevel.WORKSPACE
  );

  if (workspaceUserInfo && !canView) {
    return <NotAuthorizedView section="general" className="h-auto" />;
  }

  return (
    <div className="flex h-full w-full flex-col">
      <div className="flex-1 overflow-auto">
        <Outlet />
      </div>
    </div>
  );
});

export default PromptsLayout;
