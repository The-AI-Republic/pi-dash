// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project contracts (GET /api/workspaces/<slug>/projects/).
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { nullableDateTime, nullableUuid, uuid } from "./fields.js";

/** Small project projection embedded in members and activity. */
export const ProjectLite = z.object({
  id: uuid(),
  name: z.string(),
  identifier: z.string(),
});
export type ProjectLite = z.infer<typeof ProjectLite>;

/**
 * Project list row. The endpoint answers a fixed `.values()` projection —
 * not the full serializer — with the caller's membership role, the pending
 * intake count and the view flags annotated. Anything else the server adds
 * passes through untouched.
 */
export const Project = z.object({
  id: uuid(),
  name: z.string(),
  identifier: z.string(),
  sort_order: z.nullable(z.number()),
  logo_props: z.record(z.string(), z.unknown()),
  member_role: z.nullable(z.number()),
  intake_count: z.number(),
  archived_at: nullableDateTime(),
  workspace: uuid(),
  cycle_view: z.boolean(),
  issue_views_view: z.boolean(),
  module_view: z.boolean(),
  page_view: z.boolean(),
  inbox_view: z.boolean(),
  is_default: z.boolean(),
  guest_view_all_features: z.boolean(),
  project_lead: nullableUuid(),
  network: z.number(),
  created_at: z.iso.datetime(),
  updated_at: z.iso.datetime(),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
});
export type Project = z.infer<typeof Project>;

export const ProjectList = z.array(Project);
export type ProjectList = z.infer<typeof ProjectList>;

export async function listProjects(client: ApiClient, slug: string): Promise<ProjectList> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/projects/`);
  return client.parse(ProjectList, body, `GET /api/workspaces/${slug}/projects/`);
}

const { intake_count: _listOnly, ...projectDetailShape } = Project.shape;

/**
 * One project row answered by GET .../projects/<id>/ (the retrieve view
 * serializes the project row, not the list `.values()` projection, so the
 * list-only pending-intake count is absent). Extra serializer fields pass
 * through untouched. The denial screens read this for their fetch status:
 * unknown projects 404, private projects 403 for non-members, other
 * non-member projects 409.
 */
export const ProjectDetail = z.object(projectDetailShape);
export type ProjectDetail = z.infer<typeof ProjectDetail>;

/** GET /api/workspaces/<slug>/projects/<projectId>/ — one project row. */
export async function getProject(client: ApiClient, slug: string, projectId: string): Promise<ProjectDetail> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(ProjectDetail, body, `GET ${path}`);
}

/**
 * Full project row answered by GET .../projects/details/ and by POST
 * .../projects/ on create: every Project model field plus the membership
 * annotations (favorite flag, sort order, role, deploy-board anchor, member
 * ids) the list serializer adds.
 */
export const ProjectDetails = z.object({
  id: uuid(),
  created_at: z.iso.datetime(),
  updated_at: z.iso.datetime(),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
  deleted_at: nullableDateTime(),
  workspace: uuid(),
  name: z.string(),
  description: z.string(),
  description_text: z.nullable(z.record(z.string(), z.unknown())),
  description_html: z.nullable(z.record(z.string(), z.unknown())),
  network: z.number(),
  identifier: z.string(),
  default_assignee: nullableUuid(),
  project_lead: nullableUuid(),
  emoji: z.nullable(z.string()),
  icon_prop: z.nullable(z.record(z.string(), z.unknown())),
  module_view: z.boolean(),
  cycle_view: z.boolean(),
  issue_views_view: z.boolean(),
  page_view: z.boolean(),
  intake_view: z.boolean(),
  inbox_view: z.boolean(),
  is_time_tracking_enabled: z.boolean(),
  is_issue_type_enabled: z.boolean(),
  is_default: z.boolean(),
  guest_view_all_features: z.boolean(),
  members_can_edit_states: z.boolean(),
  cover_image: z.nullable(z.string()),
  cover_image_asset: nullableUuid(),
  cover_image_url: z.nullable(z.string()),
  estimate: nullableUuid(),
  archive_in: z.number(),
  close_in: z.number(),
  logo_props: z.record(z.string(), z.unknown()),
  default_state: nullableUuid(),
  archived_at: nullableDateTime(),
  timezone: z.string(),
  external_source: z.nullable(z.string()),
  external_id: z.nullable(z.string()),
  repo_url: z.string(),
  base_branch: z.string(),
  is_favorite: z.boolean(),
  sort_order: z.nullable(z.number()),
  member_role: z.nullable(z.number()),
  anchor: z.nullable(z.string()),
  members: z.array(uuid()),
  next_work_item_sequence: z.number(),
  agent_executor_options: z.unknown(),
});
export type ProjectDetails = z.infer<typeof ProjectDetails>;

export const ProjectDetailsList = z.array(ProjectDetails);
export type ProjectDetailsList = z.infer<typeof ProjectDetailsList>;

/** GET /api/workspaces/<slug>/projects/details/ — the rich project rows. */
export async function listProjectDetails(client: ApiClient, slug: string): Promise<ProjectDetailsList> {
  const path = `/api/workspaces/${slug}/projects/details/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(ProjectDetailsList, body, `GET ${path}`);
}

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
  emoji?: string | null;
  repo_url?: string;
  base_branch?: string;
}

/**
 * PATCH /api/workspaces/<slug>/projects/<id>/ answers 200 with the updated
 * row in the list-serializer shape (the same shape create returns).
 * Workspace and project admins only (403 otherwise); archived projects
 * refuse with 400.
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
  created_at: z.iso.datetime(),
  updated_at: z.iso.datetime(),
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

/**
 * Workspace-level display properties for the caller (filters, layout,
 * navigation prefs). GET and PATCH
 * /api/workspaces/<slug>/user-properties/; PATCH merges the given keys.
 */
export const WorkspaceUserProperties = z.object({
  id: uuid(),
  workspace: uuid(),
  user: uuid(),
  filters: z.record(z.string(), z.unknown()),
  display_filters: z.record(z.string(), z.unknown()),
  display_properties: z.record(z.string(), z.unknown()),
  rich_filters: z.record(z.string(), z.unknown()),
  navigation_project_limit: z.number(),
  navigation_control_preference: z.string(),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
  created_at: z.iso.datetime(),
  updated_at: z.iso.datetime(),
});
export type WorkspaceUserProperties = z.infer<typeof WorkspaceUserProperties>;

export async function getWorkspaceUserProperties(client: ApiClient, slug: string): Promise<WorkspaceUserProperties> {
  const path = `/api/workspaces/${slug}/user-properties/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(WorkspaceUserProperties, body, `GET ${path}`);
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

/**
 * Project-level display properties for the caller (issue filters, layout,
 * navigation preferences, sort order). GET and PATCH
 * /api/workspaces/<slug>/projects/<id>/user-properties/. PATCH merges the
 * given keys; preference writes carry the full preferences object so
 * sibling branches survive.
 */
export const ProjectUserProperties = z.object({
  id: uuid(),
  workspace: uuid(),
  project: uuid(),
  user: uuid(),
  filters: z.record(z.string(), z.unknown()),
  display_filters: z.record(z.string(), z.unknown()),
  display_properties: z.record(z.string(), z.unknown()),
  rich_filters: z.record(z.string(), z.unknown()),
  preferences: z.record(z.string(), z.unknown()),
  sort_order: z.number(),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
  created_at: z.iso.datetime(),
  updated_at: z.iso.datetime(),
});
export type ProjectUserProperties = z.infer<typeof ProjectUserProperties>;

export async function getProjectUserProperties(
  client: ApiClient,
  slug: string,
  projectId: string
): Promise<ProjectUserProperties> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/user-properties/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(ProjectUserProperties, body, `GET ${path}`);
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

/**
 * The caller's role in each project they belong to, keyed by project id.
 * The denial screens read this to decide join versus no-access.
 */
export const ProjectRoleMap = z.record(z.string(), z.number());
export type ProjectRoleMap = z.infer<typeof ProjectRoleMap>;

/** GET /api/users/me/workspaces/<slug>/project-roles/ — my project roles. */
export async function getMyProjectRoles(client: ApiClient, slug: string): Promise<ProjectRoleMap> {
  const path = `/api/users/me/workspaces/${slug}/project-roles/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(ProjectRoleMap, body, `GET ${path}`);
}
