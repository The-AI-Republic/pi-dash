// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The Modules tab on the project features screen (parity/specs/modules.md
// §10.1, MOD-036). The tab is a project-admin surface: the settings shell
// renders the not-authorized body for members and guests and only mounts
// this body for an admin, so the switch here is the project-settings
// area's one feature switch (PSET-046), not a second control.
//
// What the modules area adds is the wake-up: the switch writes the
// project's modules flag, and every modules read that decides on that
// flag is refreshed once the write answers. Without it a list already
// visited in this session would keep its fetched-on-mount facts and show
// the old gate until a reload.

import * as React from "react";

import { useQueryClient } from "@tanstack/react-query";

import { ProjectFeatureSettings, PROJECT_FEATURES } from "../../project-features/index.js";
import { invalidateModuleLists, invalidateWorkspaceModules } from "../api/invalidate.js";
import { moduleKeys } from "../api/keys.js";

export interface ModuleFeatureSettingsProps {
  workspaceSlug: string;
  projectId: string;
}

export function ModuleFeatureSettings({ workspaceSlug, projectId }: ModuleFeatureSettingsProps): React.ReactElement {
  const queryClient = useQueryClient();

  const afterFlip = React.useCallback(() => {
    void invalidateModuleLists(queryClient, workspaceSlug, projectId);
    void invalidateWorkspaceModules(queryClient, workspaceSlug);
    void queryClient.invalidateQueries({ queryKey: moduleKeys.projectFacts(workspaceSlug, projectId) });
  }, [queryClient, workspaceSlug, projectId]);

  return <ProjectFeatureSettings feature={PROJECT_FEATURES.modules} onUpdated={afterFlip} />;
}
