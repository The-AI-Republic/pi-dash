// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project management contracts: lifecycle writes (create, update, delete,
// archive, leave, join), identifier checks, project invitations, favorite
// projects, and user-property writes. The shell and gates only read
// projects (see projects.ts); everything here runs from lazy routes and
// dialogs, so it stays out of the initial bundle (NEWFRONT-523, size
// gate). Response shapes shared with the reads live in projects.ts.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableDateTime, nullableUuid, uuid } from "./fields.js";
import { ProjectDetails, ProjectUserProperties, WorkspaceUserProperties } from "./projects.js";

export interface ProjectCreateInput {
  name: string;
  identifier: string;
  description?: string;
  network?: number;
  project_lead?: string | null;
  logo_props?: Record<string, unknown>;
  timezone?: string;
  cycle_view?: boolean;
  module_view?: boolean;
  issue_views_view?: boolean;
  page_view?: boolean;
  intake_view?: boolean;
}

/**
 * POST /api/workspaces/<slug>/projects/ answers 201 with the created row
 * (and makes the caller a project admin with a default state set).
 */
export async function createProject(
  client: ApiClient,
  slug: string,
  input: ProjectCreateInput
): Promise<ProjectDetails> {
  const path = `/api/workspaces/${slug}/projects/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(ProjectDetails, envelope.data, `POST ${path}`);
}

/** DELETE answers 204 with no body; resolves the response status. */
export async function deleteProject(client: ApiClient, slug: string, projectId: string): Promise<number> {
  const response = await client.delete(`/api/workspaces/${slug}/projects/${projectId}/`);
  return response.status;
}

export interface ProjectUpdateInput {
  name?: string;
  identifier?: string;
  description?: string;
  network?: number;
  project_lead?: string | null;
  timezone?: string;
  /** Project mark: the emoji/icon/uploaded-emblem object the row stores. */
  logo_props?: Record<string, unknown>;
  emoji?: string | null;
  repo_url?: string;
  base_branch?: string;
  /** Saved-views area switch (PSET-043). */
  issue_views_view?: boolean;
  /** Timeboxing-periods area switch (PSET-044). */
  cycle_view?: boolean;
  /** Triage-inbox area switch (PSET-045). */
  inbox_view?: boolean;
  /** Sub-project grouping area switch (PSET-046). */
  module_view?: boolean;
  /** Free-form documents area switch (PSET-047). */
  page_view?: boolean;
  /** Default executor for new runs; existing runs keep their own (PSET-009). */
  default_agent_executor?: string;
  /** Master switch for machine-started runs (PSET-009). */
  agent_ticking_enabled?: boolean;
  /** Run pool per work item; -1 is the no-cap sentinel (PSET-009). */
  agent_default_max_ticks?: number;
  /** Cadence per ticking stage; the General picker writes all three. */
  agent_default_interval_seconds?: number;
  agent_review_default_interval_seconds?: number;
  agent_test_default_interval_seconds?: number;
  /** Idle-archive delay in months (0 disables); the automations page. */
  archive_in?: number;
  /** Idle-close delay in months (0 disables); the automations page. */
  close_in?: number;
  /** Cancelled-state target for idle closes; a state of this project. */
  default_state?: string | null;
  /**
   * The project's active estimate system (PSET-049's enable switch): an
   * estimate id of this project turns estimation on, null turns it off.
   */
  estimate?: string | null;
  /**
   * Whether project members may edit workflow states (PSET-034). Workspace
   * and project admins always may; this flag widens that to members. The
   * states tab flips it through this same project update.
   */
  members_can_edit_states?: boolean;
}

/**
 * PATCH /api/workspaces/<slug>/projects/<id>/ answers 200 with the updated
 * row in the list-serializer shape (the same shape create returns).
 * Workspace and project admins only (403 otherwise); archived projects
 * refuse with 400. `default_state` must be a state of this project.
 */
export async function updateProject(
  client: ApiClient,
  slug: string,
  projectId: string,
  input: ProjectUpdateInput
): Promise<ProjectDetails> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: input });
  return client.parse(ProjectDetails, envelope.data, `PATCH ${path}`);
}

/**
 * The project row the settings General tab reads (PSET-005). It is the same
 * serializer as ProjectDetails, plus the agent-policy columns the tab owns
 * that the shared projection leaves off; the server sends the whole model.
 */
export const ProjectSettingsDetail = z.object({
  ...ProjectDetails.shape,
  agent_ticking_enabled: z.boolean(),
  agent_default_max_ticks: z.number(),
  agent_default_interval_seconds: z.number(),
  agent_review_default_interval_seconds: z.number(),
  agent_test_default_interval_seconds: z.number(),
});
export type ProjectSettingsDetail = z.infer<typeof ProjectSettingsDetail>;

/** GET /api/workspaces/<slug>/projects/<id>/ — the General tab's read. */
export async function getProjectSettingsDetail(
  client: ApiClient,
  slug: string,
  projectId: string
): Promise<ProjectSettingsDetail> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(ProjectSettingsDetail, body, `GET ${path}`);
}

/**
 * The same PATCH as updateProject, read back with the policy columns the
 * General tab owns. The server answers with the whole model, so the two
 * parsers differ only in how much of the answer each caller needs.
 */
export async function updateProjectSettings(
  client: ApiClient,
  slug: string,
  projectId: string,
  input: ProjectUpdateInput
): Promise<ProjectSettingsDetail> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: input });
  return client.parse(ProjectSettingsDetail, envelope.data, `PATCH ${path}`);
}

/** One reserved identifier row: integer id, upper-cased name, owning project. */
export const ProjectIdentifierMatch = z.object({
  id: z.number(),
  name: z.string(),
  project: uuid(),
});
export type ProjectIdentifierMatch = z.infer<typeof ProjectIdentifierMatch>;

/**
 * Identifier-availability answer: `exists` counts the reservations for the
 * name and `identifiers` carries them (empty when the name is free). The
 * server upper-cases the query; a missing name 400s.
 */
export const ProjectIdentifierCheck = z.object({
  exists: z.number(),
  identifiers: z.array(ProjectIdentifierMatch),
});
export type ProjectIdentifierCheck = z.infer<typeof ProjectIdentifierCheck>;

/** GET /api/workspaces/<slug>/project-identifiers/?name= — availability check. */
export async function checkProjectIdentifier(
  client: ApiClient,
  slug: string,
  name: string
): Promise<ProjectIdentifierCheck> {
  const path = `/api/workspaces/${slug}/project-identifiers/`;
  const body = await client.getJson<unknown>(path, { query: { name } });
  return client.parse(ProjectIdentifierCheck, body, `GET ${path}`);
}

export const ProjectArchiveResult = z.object({
  archived_at: z.string(),
});
export type ProjectArchiveResult = z.infer<typeof ProjectArchiveResult>;

/**
 * POST .../projects/<id>/archive/ stamps `archived_at` (and clears the
 * project's favorites); DELETE .../archive/ restores it with a 204.
 */
export async function archiveProject(
  client: ApiClient,
  slug: string,
  projectId: string
): Promise<ProjectArchiveResult> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/archive/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: {} });
  return client.parse(ProjectArchiveResult, envelope.data, `POST ${path}`);
}

export async function unarchiveProject(client: ApiClient, slug: string, projectId: string): Promise<number> {
  const response = await client.delete(`/api/workspaces/${slug}/projects/${projectId}/archive/`);
  return response.status;
}

/**
 * POST .../projects/<id>/members/leave/ deactivates the caller's
 * membership (204); the only admin cannot leave. Resolves the status.
 */
export async function leaveProject(client: ApiClient, slug: string, projectId: string): Promise<number> {
  const response = await client.post(`/api/workspaces/${slug}/projects/${projectId}/members/leave/`, {});
  return response.status;
}

/** Nested project projection inside invitation rows. */
export const InvitationProject = z.object({
  id: uuid(),
  identifier: z.string(),
  name: z.string(),
  cover_image: z.nullable(z.string()),
  cover_image_url: z.nullable(z.string()),
  logo_props: z.record(z.string(), z.unknown()),
  description: z.string(),
  is_default: z.boolean(),
});
export type InvitationProject = z.infer<typeof InvitationProject>;

export const InvitationWorkspace = z.object({
  id: uuid(),
  name: z.string(),
  slug: z.string(),
  logo_url: z.nullable(z.string()),
});
export type InvitationWorkspace = z.infer<typeof InvitationWorkspace>;

/** Project invitation row for the caller's email with nested project/workspace. */
export const ProjectInvitation = z.object({
  id: uuid(),
  project: InvitationProject,
  workspace: InvitationWorkspace,
  email: z.string(),
  accepted: z.boolean(),
  token: z.string(),
  message: z.nullable(z.string()),
  responded_at: nullableDateTime(),
  role: z.number(),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
  created_at: dateTime(),
  updated_at: dateTime(),
});
export type ProjectInvitation = z.infer<typeof ProjectInvitation>;

export const ProjectInvitationList = z.array(ProjectInvitation);
export type ProjectInvitationList = z.infer<typeof ProjectInvitationList>;

/** GET /api/users/me/workspaces/<slug>/projects/invitations/ — my pending invites. */
export async function listProjectInvitations(client: ApiClient, slug: string): Promise<ProjectInvitationList> {
  const path = `/api/users/me/workspaces/${slug}/projects/invitations/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(ProjectInvitationList, body, `GET ${path}`);
}

export const JoinProjectsResult = z.object({
  message: z.string(),
});
export type JoinProjectsResult = z.infer<typeof JoinProjectsResult>;

/**
 * POST /api/users/me/workspaces/<slug>/projects/invitations/ with
 * `{ project_ids }` joins the caller to each project (201 + message).
 */
export async function joinProjects(client: ApiClient, slug: string, projectIds: string[]): Promise<JoinProjectsResult> {
  const path = `/api/users/me/workspaces/${slug}/projects/invitations/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: { project_ids: projectIds } });
  return client.parse(JoinProjectsResult, envelope.data, `POST ${path}`);
}

/**
 * Favorite-project row. `entity_data` is the favorited project's own
 * serialized shape, null when the project is gone.
 */
export const FavoriteProject = z.object({
  id: uuid(),
  entity_type: z.string(),
  entity_identifier: z.string(),
  entity_data: z.nullable(z.record(z.string(), z.unknown())),
  name: z.string(),
  is_folder: z.boolean(),
  sequence: z.number(),
  parent: nullableUuid(),
  workspace_id: uuid(),
  project_id: nullableUuid(),
});
export type FavoriteProject = z.infer<typeof FavoriteProject>;

export const FavoriteProjectList = z.array(FavoriteProject);
export type FavoriteProjectList = z.infer<typeof FavoriteProjectList>;

/** GET /api/workspaces/<slug>/user-favorite-projects/ — my favorites. */
export async function listFavoriteProjects(client: ApiClient, slug: string): Promise<FavoriteProjectList> {
  const path = `/api/workspaces/${slug}/user-favorite-projects/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(FavoriteProjectList, body, `GET ${path}`);
}

/**
 * POST adds a favorite (`{ project: <id> }`, 204) and
 * DELETE .../user-favorite-projects/<project_id>/ removes it (204).
 * Both resolve the response status.
 */
export async function addFavoriteProject(client: ApiClient, slug: string, projectId: string): Promise<number> {
  const envelope = await client.sendJson<unknown>("POST", `/api/workspaces/${slug}/user-favorite-projects/`, {
    json: { project: projectId },
  });
  return envelope.status;
}

export async function removeFavoriteProject(client: ApiClient, slug: string, projectId: string): Promise<number> {
  const response = await client.delete(`/api/workspaces/${slug}/user-favorite-projects/${projectId}/`);
  return response.status;
}

export async function updateWorkspaceUserProperties(
  client: ApiClient,
  slug: string,
  patch: Partial<Pick<WorkspaceUserProperties, "navigation_project_limit" | "navigation_control_preference">>
): Promise<WorkspaceUserProperties> {
  const path = `/api/workspaces/${slug}/user-properties/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: patch });
  return client.parse(WorkspaceUserProperties, envelope.data, `PATCH ${path}`);
}

export async function updateProjectUserProperties(
  client: ApiClient,
  slug: string,
  projectId: string,
  patch: Partial<Pick<ProjectUserProperties, "sort_order" | "preferences">>
): Promise<ProjectUserProperties> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/user-properties/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: patch });
  return client.parse(ProjectUserProperties, envelope.data, `PATCH ${path}`);
}
