// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Feature-switch writes (parity/specs/project-settings-part-f.md,
// PSET-043–047). One PATCH of one boolean. The answering row is folded
// into every project cache that carries the flags — the retrieve row the
// settings tabs read, the rich collection, and the project-list
// collection the sidebar and the tab strip read — so the switch, the
// project tab strip and the sidebar agree the moment the write lands.
// The project-list row is the endpoint's own projection (PSET-043–047
// reuse the list's flag fields), so only the flags travel into it and
// its pending-intake count and role stay untouched. The pending notice is
// settled by the caller so a failed write leaves the stored value on
// screen.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  updateProject,
  type ProjectDetail,
  type ProjectDetails,
  type ProjectList,
  type ProjectUpdateInput,
} from "@pidash/api-client";

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
 * The list row after one flag write: the patch's flags win, every other
 * list field stays. Other rows, and an absent collection, come back
 * unchanged so a cold sidebar never starts holding a partial row.
 */
export function foldFeatureIntoList(
  rows: ProjectList | undefined,
  projectId: string,
  patch: FeaturePatch
): ProjectList | undefined {
  return rows?.map((row) => (row.id === projectId ? { ...row, ...patch } : row));
}

/**
 * PATCH one project boolean and fold the answered row into the retrieve
 * cache, the collection and the list. Both collection reads share the
 * server's serialization, so a field one omits simply stays absent there.
 */
export function useUpdateProjectFeature(workspaceSlug: string) {
  const queryClient = useQueryClient();
  return useMutation<ProjectDetails, Error, UpdateFeatureInput>({
    mutationFn: ({ projectId, patch }) => updateProject(getAppClient(), workspaceSlug, projectId, patch),
    onSuccess: (project, { projectId, patch }) => {
      queryClient.setQueryData(authKeys.project(workspaceSlug, projectId), (row: ProjectDetail | undefined) =>
        row === undefined ? row : { ...row, ...project }
      );
      queryClient.setQueryData(projectKeys.details(workspaceSlug), (rows: ProjectDetails[] | undefined) =>
        rows?.map((entry) => (entry.id === projectId ? project : entry))
      );
      // The sidebar and the project tab strip read this collection with a
      // long stale time: without the fold the area link outlives the
      // write until a reload (PSET-043–047).
      queryClient.setQueryData(projectKeys.list(workspaceSlug), (rows: ProjectList | undefined) =>
        foldFeatureIntoList(rows, projectId, patch)
      );
    },
  });
}
