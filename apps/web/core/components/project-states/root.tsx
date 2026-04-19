/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useMemo } from "react";
import { observer } from "mobx-react";
import useSWR from "swr";
// components
import { EUserPermissionsLevel } from "@pi-dash/constants";
import type { IState, TStateOperationsCallbacks } from "@pi-dash/types";
import { EUserProjectRoles } from "@pi-dash/types";
import { ProjectStateLoader, GroupList } from "@/components/project-states";
// hooks
import { useProject } from "@/hooks/store/use-project";
import { useProjectState } from "@/hooks/store/use-project-state";
import { useUserPermissions } from "@/hooks/store/user";

type TProjectState = {
  workspaceSlug: string;
  projectId: string;
};

export const ProjectStateRoot = observer(function ProjectStateRoot(props: TProjectState) {
  const { workspaceSlug, projectId } = props;
  // hooks
  const {
    groupedProjectStates,
    fetchProjectStates,
    createState,
    moveStatePosition,
    updateState,
    deleteState,
    markStateAsDefault,
  } = useProjectState();
  const { allowPermissions } = useUserPermissions();
  const { getProjectById } = useProject();
  // derived values
  const isAdmin = allowPermissions([EUserProjectRoles.ADMIN], EUserPermissionsLevel.PROJECT, workspaceSlug, projectId);
  const isMember = allowPermissions(
    [EUserProjectRoles.MEMBER],
    EUserPermissionsLevel.PROJECT,
    workspaceSlug,
    projectId
  );
  const membersCanEditStates = getProjectById(projectId)?.members_can_edit_states ?? true;
  const isEditable = isAdmin || (isMember && membersCanEditStates);

  // Fetching all project states
  useSWR(
    workspaceSlug && projectId ? `PROJECT_STATES_${workspaceSlug}_${projectId}` : null,
    workspaceSlug && projectId ? () => fetchProjectStates(workspaceSlug.toString(), projectId.toString()) : null,
    { revalidateIfStale: false, revalidateOnFocus: false }
  );

  // State operations callbacks
  const stateOperationsCallbacks: TStateOperationsCallbacks = useMemo(
    () => ({
      createState: async (data: Partial<IState>) => createState(workspaceSlug, projectId, data),
      updateState: async (stateId: string, data: Partial<IState>) =>
        updateState(workspaceSlug, projectId, stateId, data),
      deleteState: async (stateId: string) => deleteState(workspaceSlug, projectId, stateId),
      moveStatePosition: async (stateId: string, data: Partial<IState>) =>
        moveStatePosition(workspaceSlug, projectId, stateId, data),
      markStateAsDefault: async (stateId: string) => markStateAsDefault(workspaceSlug, projectId, stateId),
    }),
    [workspaceSlug, projectId, createState, moveStatePosition, updateState, deleteState, markStateAsDefault]
  );

  // Loader
  if (!groupedProjectStates) return <ProjectStateLoader />;

  return (
    <GroupList
      groupedStates={groupedProjectStates}
      stateOperationsCallbacks={stateOperationsCallbacks}
      isEditable={isEditable}
      shouldTrackEvents
    />
  );
});
