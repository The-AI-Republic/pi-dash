// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Feature-switch writes (parity/specs/project-settings-part-f.md,
// PSET-043–047). One PATCH of one boolean. The answering row is folded
// into both project caches that carry the flags — the retrieve row the
// settings tabs read and the collection the project navigation reads —
// so the switch, the project tab strip and the sidebar agree the moment
// the write lands. The pending notice is settled by the caller so a
// failed write leaves the stored value on screen.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { updateProject, type ProjectDetail, type ProjectDetails, type ProjectUpdateInput } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { authKeys } from "../../auth/index.js";
import { projectKeys } from "../../projects/index.js";

/** Heading of the refusal body non-admins see on a feature tab. */
export const FEATURE_NOT_AUTHORIZED_TITLE = "Not authorized to change features";
export const FEATURE_NOT_AUTHORIZED_BODY = "Only project admins can turn project features on or off.";

/** Inside a feature tab, above the switch. */
export const FEATURE_SETTINGS_CONTEXT = "Project settings";
/** Explainers shared by every tab and probe text. */
export const FEATURE_UPDATE_PENDING_TITLE = "Updating project feature";
export const FEATURE_UPDATE_PENDING_MESSAGE = "Applying the change…";
export const FEATURE_UPDATE_SUCCESS_TITLE = "Project feature updated";
export const FEATURE_UPDATE_SUCCESS_MESSAGE = "Project feature updated successfully.";
export const FEATURE_UPDATE_FAILED_TITLE = "Error!";
export const FEATURE_UPDATE_FAILED_MESSAGE = "Something went wrong while updating project feature. Please try again.";

/** The single-field payload a switch writes. */
export type FeaturePatch = Pick<
  ProjectUpdateInput,
  "issue_views_view" | "cycle_view" | "inbox_view" | "module_view" | "page_view"
>;

export interface UpdateFeatureInput {
  projectId: string;
  patch: FeaturePatch;
}

/**
 * PATCH one project boolean and fold the answered row into the retrieve
 * cache and the collection. Both reads share the server's serialization,
 * so a field the collection omits simply stays absent there.
 */
export function useUpdateProjectFeature(workspaceSlug: string) {
  const queryClient = useQueryClient();
  return useMutation<ProjectDetails, Error, UpdateFeatureInput>({
    mutationFn: ({ projectId, patch }) => updateProject(getAppClient(), workspaceSlug, projectId, patch),
    onSuccess: (project) => {
      queryClient.setQueryData(authKeys.project(workspaceSlug, project.id), (row: ProjectDetail | undefined) =>
        row === undefined ? row : { ...row, ...project }
      );
      queryClient.setQueryData(projectKeys.details(workspaceSlug), (rows: ProjectDetails[] | undefined) =>
        rows?.map((entry) => (entry.id === project.id ? project : entry))
      );
    },
  });
}
