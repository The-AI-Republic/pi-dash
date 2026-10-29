/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import { useParams } from "react-router";
// components
import { SettingsSidebarWorkspaceSettingGroups } from "@/components/settings/sidebar/workspace-setting-groups";

// The item list itself lives in `SettingsSidebarWorkspaceSettingGroups`, shared with the
// per-workspace groups nested in the account settings navigator. This wrapper only supplies
// the slug from the route.
export const WorkspaceSettingsSidebarItemCategories = observer(function WorkspaceSettingsSidebarItemCategories() {
  // params
  const { workspaceSlug } = useParams();

  return (
    <SettingsSidebarWorkspaceSettingGroups
      workspaceSlug={workspaceSlug ?? ""}
      className="mt-3 flex flex-col divide-y divide-subtle px-3"
      categoryClassName="py-3 first:pt-0 last:pb-0"
    />
  );
});
