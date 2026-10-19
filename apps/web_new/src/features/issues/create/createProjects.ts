// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project-selection rules for the create entry points (ISS-117/ISS-123).
// The shell-level `ni` registration and the sidebar entry ask which
// projects may receive a new work item on every page, so these helpers
// live apart from the dialog's form shaping: importing the shell entry
// must not pull the dialog's editor and picker helpers into the initial
// bundle (size gate).

import { ROLE_MEMBER } from "../../../core/session/permissions.js";

/** Longest title the API and the dialog accept (ISS-118). */
export const TITLE_MAX_LENGTH = 255;

/** A project row the create entry points offer. */
export interface CreateProjectOption {
  id: string;
  name: string;
  identifier: string;
  memberRole: number | null;
  archived: boolean;
}

/** A caller's project row, before the optional fields are filled in. */
export interface CreateProjectLike {
  id: string;
  name: string;
  identifier?: string | undefined;
  memberRole?: number | null | undefined;
  archived?: boolean | undefined;
}

/** Fill the optional identifier, role and archive flag defaults. */
export function toCreateProjectOption(project: CreateProjectLike): CreateProjectOption {
  return {
    id: project.id,
    name: project.name,
    identifier: project.identifier ?? project.name.slice(0, 3).toUpperCase(),
    memberRole: project.memberRole ?? null,
    archived: project.archived ?? false,
  };
}

/**
 * Projects that may receive a new work item (ISS-117/ISS-123): joined
 * member-or-admin projects, minus archived ones. A guest membership never
 * appears; the dialog refuses to render when the list is empty.
 */
export function selectCreatableProjects(projects: readonly CreateProjectOption[]): CreateProjectOption[] {
  return projects.filter(
    (project) => !project.archived && project.memberRole !== null && project.memberRole >= ROLE_MEMBER
  );
}

/** Projects the entry points may offer, from a caller's own rows. */
export function selectCreatableWorkItemProjects(projects: readonly CreateProjectLike[]): CreateProjectOption[] {
  return selectCreatableProjects(projects.map(toCreateProjectOption));
}

