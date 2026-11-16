// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archives project reads (parity/specs/archives.md §1). One hook for the
// header: the project row (name plus the cycle/module flags) with the
// joined-projects switcher entries beside it.

import { useQuery } from "@tanstack/react-query";
import type { ProjectDetail } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { useProjectRow } from "../../auth/index.js";
import { projectsQueryOptions, type SwitcherProject } from "../../projects/index.js";

export interface ArchivesProject {
  project: ProjectDetail;
  joined: SwitcherProject[];
}

/**
 * Project row plus joined-projects switcher entries. Null while the row
 * resolves or when it errors; the project gate owns the denial screens,
 * so the shell renders nothing instead of its own denied state.
 */
export function useArchivesProject(workspaceSlug: string, projectId: string): ArchivesProject | null {
  const row = useProjectRow(workspaceSlug, projectId);
  const projects = useQuery(projectsQueryOptions(getAppClient(), workspaceSlug));
  if (row.data === undefined) return null;
  const joined = (projects.data ?? [])
    .filter((entry) => entry.member_role !== null)
    .map((entry) => ({ id: entry.id, name: entry.name }));
  return { project: row.data, joined };
}
