// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// List section names and strip values (parity/specs/issues.md §2,
// ISS-007/014). Group dimensions resolve ids to human names through the
// same reference reads the pickers use; the strip resolves a row's
// assignees, labels, modules and cycle without extra per-row fetches.

import type { CycleList, LabelList, ModuleList, ProjectList, State } from "@pidash/api-client";
import * as React from "react";

import { useMe } from "../../../core/session/index.js";
import { useLabels } from "../labels/api.js";
// Direct module imports keep the picker UI components out of the list's
// first-route chunk; only the reference reads and option shaping load here.
import { selectCycleOptions } from "../../../shared/pickers/cycles.js";
import { selectMemberOptions } from "../../../shared/pickers/members.js";
import {
  usePickerCycles,
  usePickerModules,
  usePickerProjectMembers,
  usePickerProjects,
  usePickerStates,
  usePickerWorkspaceMembers,
} from "../../../shared/pickers/reference.js";

/** Dimension → id-to-name map for section titles. */
export type GroupLabels = Partial<Record<string, ReadonlyMap<string, string>>>;

export interface ListReferenceInput {
  workspaceSlug: string;
  projectId: string;
  /** Fetch grouping references; the strip reads states regardless. */
  dimension: string | null;
}

export interface ListReferences {
  labels: GroupLabels;
  /** Workflow states in server order (the strip reads names and glyphs). */
  stateRows: State[];
  /** Name lookups the strip uses on each row. */
  stateNames: ReadonlyMap<string, string>;
  assigneeNames: ReadonlyMap<string, string>;
  labelNames: ReadonlyMap<string, string>;
  cycleNames: ReadonlyMap<string, string>;
  moduleNames: ReadonlyMap<string, string>;
  projectNames: ReadonlyMap<string, string>;
  /** Cycles still open; completed cycles render read-only. */
  openCycleIds: ReadonlySet<string>;
  projects: ProjectList;
  isLoading: boolean;
}

function nameMap(rows: ReadonlyArray<{ id: string; name: string }>): Map<string, string> {
  return new Map(rows.map((row) => [row.id, row.name]));
}

/**
 * Reference data for list sections and the inline strip. Grouping is the
 * only consumer of most dimensions, but the strip always needs states
 * (names plus the group behind the overdue flag) and projects (the
 * identifier prefix).
 */
export function useListReferences({ workspaceSlug, projectId, dimension }: ListReferenceInput): ListReferences {
  const needs = (key: string): boolean => dimension === null || dimension === key;
  const states = usePickerStates(workspaceSlug, projectId, true);
  const members = usePickerProjectMembers(workspaceSlug, projectId, needs("assignees"));
  const directory = usePickerWorkspaceMembers(workspaceSlug, needs("assignees"));
  const projects = usePickerProjects(workspaceSlug, needs("project") || needs("team_project"));
  const cycles = usePickerCycles(workspaceSlug, projectId, needs("cycle"));
  const modules = usePickerModules(workspaceSlug, projectId, needs("module"));
  const labels = useLabels(workspaceSlug, projectId, needs("labels"));
  const me = useMe();

  const stateRows = states.data ?? [];
  const stateNames = React.useMemo(() => nameMap(stateRows), [stateRows]);

  const memberRows = React.useMemo(() => {
    const options = selectMemberOptions({
      projectMembers: members.data,
      directory: directory.data ?? [],
      meId: me.data?.id ?? null,
      selectedIds: [],
    });
    return options.map((option) => ({ id: option.userId, name: option.searchText }));
  }, [members.data, directory.data, me.data?.id]);
  const assigneeNames = React.useMemo(() => nameMap(memberRows), [memberRows]);

  const labelRows: LabelList = labels.data ?? [];
  const labelNames = React.useMemo(() => nameMap(labelRows), [labelRows]);

  const cycleRows: CycleList = cycles.data ?? [];
  const cycleNames = React.useMemo(() => nameMap(cycleRows), [cycleRows]);
  const openCycleIds = React.useMemo(() => {
    const open = selectCycleOptions({ cycles: cycleRows, allowClear: false });
    return new Set(open.map((option) => option.id));
  }, [cycleRows]);

  const moduleRows: ModuleList = modules.data ?? [];
  const moduleNames = React.useMemo(() => nameMap(moduleRows), [moduleRows]);

  const projectRows: ProjectList = projects.data ?? [];
  const projectNames = React.useMemo(() => nameMap(projectRows), [projectRows]);

  const labelsByDimension: GroupLabels = {
    state: stateNames,
    labels: labelNames,
    assignees: assigneeNames,
    cycle: cycleNames,
    module: moduleNames,
    project: projectNames,
    team_project: projectNames,
  };
  if (dimension === "created_by") {
    const creators = new Map<string, string>();
    for (const option of selectMemberOptions({
      directory: directory.data ?? [],
      meId: me.data?.id ?? null,
      selectedIds: [],
    })) {
      creators.set(option.userId, option.searchText);
    }
    labelsByDimension.created_by = creators;
  }

  return {
    labels: labelsByDimension,
    stateRows,
    stateNames,
    assigneeNames,
    labelNames,
    cycleNames,
    moduleNames,
    projectNames,
    openCycleIds,
    projects: projectRows,
    isLoading:
      states.isPending ||
      (needs("project") || needs("team_project") ? projects.isPending : false) ||
      labels.isPending ||
      (needs("assignees") && (members.isPending || directory.isPending)) ||
      (needs("cycle") && cycles.isPending) ||
      (needs("module") && modules.isPending),
  };
}
