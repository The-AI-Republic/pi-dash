// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Modules: project-scoped containers for work items, with their links,
// favorites, archive/restore writes and archived reads. Every module read
// renders the same row projection, so one row schema serves the project
// list, the workspace set and the archived list; the detail adds the
// distribution the progress chart and breakdowns read. Work-item lists and
// membership writes stay with the Issues contracts (`listModuleIssues` and
// `listGroupedModuleIssues` in issues.ts, `addIssuesToModule` and
// `setIssueModules` in issue-actions.ts); this file carries their path
// builders and the one removal call the module surface owns.
//
// Dates: `start_date` and `target_date` are calendar dates. Audit stamps are
// datetimes and may carry a numeric offset. `description_html` and
// `description_text` are the editor's opaque serialized values: module
// surfaces pass them back to the editor and never inspect their contents.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { ArchivedDistribution, ArchivedEstimateDistribution } from "./cycles.js";
import { dateTime, nullableDate, nullableDateTime, nullableUuid, uuid } from "./fields.js";

/** Workflow states a module can carry, in picker order. */
export const MODULE_STATUSES = ["backlog", "planned", "in-progress", "paused", "completed", "cancelled"] as const;
export type ModuleStatus = (typeof MODULE_STATUSES)[number];

/** The only statuses the server lets a module be archived from. */
export const ARCHIVABLE_MODULE_STATUSES: readonly ModuleStatus[] = ["completed", "cancelled"];

// ---------------------------------------------------------------- paths

export function modulesCollection(slug: string, projectId: string): string {
  return `/api/workspaces/${slug}/projects/${projectId}/modules/`;
}

export function moduleDetailPath(slug: string, projectId: string, moduleId: string): string {
  return `${modulesCollection(slug, projectId)}${moduleId}/`;
}

export function moduleIssuesPath(slug: string, projectId: string, moduleId: string): string {
  return `${moduleDetailPath(slug, projectId, moduleId)}issues/`;
}

export function moduleIssuePath(slug: string, projectId: string, moduleId: string, issueId: string): string {
  return `${moduleIssuesPath(slug, projectId, moduleId)}${issueId}/`;
}

export function moduleLinksPath(slug: string, projectId: string, moduleId: string): string {
  return `${moduleDetailPath(slug, projectId, moduleId)}module-links/`;
}

export function moduleLinkPath(slug: string, projectId: string, moduleId: string, linkId: string): string {
  return `${moduleLinksPath(slug, projectId, moduleId)}${linkId}/`;
}

export function moduleFavoritesPath(slug: string, projectId: string): string {
  return `/api/workspaces/${slug}/projects/${projectId}/user-favorite-modules/`;
}

export function moduleFavoritePath(slug: string, projectId: string, moduleId: string): string {
  return `${moduleFavoritesPath(slug, projectId)}${moduleId}/`;
}

export function moduleArchivePath(slug: string, projectId: string, moduleId: string): string {
  return `${moduleDetailPath(slug, projectId, moduleId)}archive/`;
}

export function archivedModulesPath(slug: string, projectId: string): string {
  return `/api/workspaces/${slug}/projects/${projectId}/archived-modules/`;
}

export function archivedModulePath(slug: string, projectId: string, moduleId: string): string {
  return `${archivedModulesPath(slug, projectId)}${moduleId}/`;
}

export function workspaceModulesPath(slug: string): string {
  return `/api/workspaces/${slug}/modules/`;
}

// ---------------------------------------------------------------- schemas

/**
 * The module row every read renders. Counts are the annotations the server
 * stamps on each module query, so a parsed row always carries them. Fields
 * only some projections add (estimate sums, archive stamp, editor payloads,
 * audit authors) stay optional: the archived list, the list and the detail
 * do not all send the same subset.
 */
const moduleRowFields = {
  id: uuid(),
  workspace_id: uuid(),
  project_id: uuid(),
  name: z.string(),
  description: z.string(),
  description_text: z.optional(z.unknown()),
  description_html: z.optional(z.unknown()),
  start_date: nullableDate(),
  target_date: nullableDate(),
  status: z.enum(MODULE_STATUSES),
  lead_id: nullableUuid(),
  member_ids: z.array(uuid()),
  view_props: z.optional(z.record(z.string(), z.unknown())),
  logo_props: z.optional(z.record(z.string(), z.unknown())),
  sort_order: z.number(),
  external_source: z.optional(z.nullable(z.string())),
  external_id: z.optional(z.nullable(z.string())),
  is_favorite: z.boolean(),
  total_issues: z.number(),
  completed_issues: z.number(),
  cancelled_issues: z.number(),
  started_issues: z.number(),
  unstarted_issues: z.number(),
  backlog_issues: z.number(),
  total_estimate_points: z.optional(z.number()),
  completed_estimate_points: z.optional(z.number()),
  backlog_estimate_points: z.optional(z.number()),
  unstarted_estimate_points: z.optional(z.number()),
  started_estimate_points: z.optional(z.number()),
  cancelled_estimate_points: z.optional(z.number()),
  created_at: dateTime(),
  updated_at: dateTime(),
  archived_at: z.optional(nullableDateTime()),
  created_by: z.optional(nullableUuid()),
  updated_by: z.optional(nullableUuid()),
};

/** One module row as the project list, workspace set and archived list send it. */
export const ModuleRow = z.object(moduleRowFields);
export type ModuleRow = z.infer<typeof ModuleRow>;

/** Bare-array module collection. */
export const ModuleRowList = z.array(ModuleRow);
export type ModuleRowList = z.infer<typeof ModuleRowList>;

/**
 * The thin row the shell palette and the shared module picker read. It stays
 * a separate name pinned to the five fields those callers depend on; a full
 * row satisfies it.
 */
export const ModuleListItem = z.object({
  id: uuid(),
  project_id: uuid(),
  name: z.string(),
  status: z.string(),
  is_favorite: z.boolean(),
});
export type ModuleListItem = z.infer<typeof ModuleListItem>;

export const ModuleList = z.array(ModuleListItem);
export type ModuleList = z.infer<typeof ModuleList>;

/**
 * The echo of a partial update: the edited fields as the write path answers
 * them. `start_date`/`target_date` come back as calendar dates from the
 * update projection, not datetimes.
 */
export const ModuleUpdate = z.object({
  id: uuid(),
  name: z.string(),
  project_id: uuid(),
  description: z.optional(z.string()),
  start_date: nullableDate(),
  target_date: nullableDate(),
  status: z.optional(z.enum(MODULE_STATUSES)),
});
export type ModuleUpdate = z.infer<typeof ModuleUpdate>;

/** One assignee's share of a module's work items. */
export const ModuleAssigneeDistribution = z.object({
  assignee_id: nullableUuid(),
  avatar_url: z.nullable(z.string()),
  first_name: z.nullable(z.string()),
  last_name: z.nullable(z.string()),
  display_name: z.nullable(z.string()),
  total_issues: z.number(),
  completed_issues: z.number(),
  pending_issues: z.number(),
});
export type ModuleAssigneeDistribution = z.infer<typeof ModuleAssigneeDistribution>;

/** One label's share of a module's work items. */
export const ModuleLabelDistribution = z.object({
  label_id: nullableUuid(),
  label_name: z.nullable(z.string()),
  color: z.nullable(z.string()),
  total_issues: z.number(),
  completed_issues: z.number(),
  pending_issues: z.number(),
});
export type ModuleLabelDistribution = z.infer<typeof ModuleLabelDistribution>;

/** Same assignee row expressed in estimate points. */
export const ModuleAssigneeEstimateDistribution = z.object({
  assignee_id: nullableUuid(),
  avatar_url: z.nullable(z.string()),
  first_name: z.nullable(z.string()),
  last_name: z.nullable(z.string()),
  display_name: z.nullable(z.string()),
  total_estimates: z.nullable(z.number()),
  completed_estimates: z.nullable(z.number()),
  pending_estimates: z.nullable(z.number()),
});
export type ModuleAssigneeEstimateDistribution = z.infer<typeof ModuleAssigneeEstimateDistribution>;

/** Same label row expressed in estimate points. */
export const ModuleLabelEstimateDistribution = z.object({
  label_id: nullableUuid(),
  label_name: z.nullable(z.string()),
  color: z.nullable(z.string()),
  total_estimates: z.nullable(z.number()),
  completed_estimates: z.nullable(z.number()),
  pending_estimates: z.nullable(z.number()),
});
export type ModuleLabelEstimateDistribution = z.infer<typeof ModuleLabelEstimateDistribution>;

/**
 * Burndown points keyed by calendar date. Dates after today carry null,
 * where the server stops the line instead of projecting a value.
 */
export const ModuleCompletionChart = z.record(z.string(), z.nullable(z.number()));
export type ModuleCompletionChart = z.infer<typeof ModuleCompletionChart>;

/** Work-item distribution behind the progress section and its tabs. */
export const ModuleDistribution = z.object({
  assignees: z.array(ModuleAssigneeDistribution),
  labels: z.array(ModuleLabelDistribution),
  completion_chart: ModuleCompletionChart,
});
export type ModuleDistribution = z.infer<typeof ModuleDistribution>;

/**
 * Estimate distribution. Only present when the project estimates in points;
 * the server otherwise answers an empty object, so every key is optional.
 */
export const ModuleEstimateDistribution = z.object({
  assignees: z.optional(z.array(ModuleAssigneeEstimateDistribution)),
  labels: z.optional(z.array(ModuleLabelEstimateDistribution)),
  completion_chart: z.optional(ModuleCompletionChart),
});
export type ModuleEstimateDistribution = z.infer<typeof ModuleEstimateDistribution>;

/** One reference link attached to a module. */
export const ModuleLink = z.object({
  id: uuid(),
  project: z.optional(uuid()),
  workspace: z.optional(uuid()),
  module: z.optional(uuid()),
  title: z.nullable(z.string()),
  url: z.string(),
  metadata: z.optional(z.record(z.string(), z.unknown())),
  created_at: z.optional(dateTime()),
  updated_at: z.optional(dateTime()),
  created_by: z.optional(nullableUuid()),
  updated_by: z.optional(nullableUuid()),
  deleted_at: z.optional(nullableDateTime()),
});
export type ModuleLink = z.infer<typeof ModuleLink>;

/** Link collection, newest first. */
export const ModuleLinkList = z.array(ModuleLink);
export type ModuleLinkList = z.infer<typeof ModuleLinkList>;

/**
 * Module detail: the row plus detail-only aggregates and the distribution
 * the progress chart reads. `link_module` repeats the links the sidebar's
 * links section lists. `sub_issues` is null when the annotation finds none.
 */
export const ModuleDetail = z.object({
  ...moduleRowFields,
  link_module: z.array(ModuleLink),
  sub_issues: z.nullable(z.number()),
  distribution: ModuleDistribution,
  estimate_distribution: ModuleEstimateDistribution,
});
export type ModuleDetail = z.infer<typeof ModuleDetail>;

/**
 * The body a full replace answers. Unlike every read, PUT falls through to
 * the framework's default update handler, which echoes the write serializer:
 * the relations arrive under their model names (`workspace`, `project`,
 * `lead`, `members`), `deleted_at` is present, and the read-only annotations
 * (favorite flag, issue counts, estimate sums) are absent. Callers that need
 * the annotated row re-read the detail after replacing.
 */
export const ModuleWriteEcho = z.object({
  id: uuid(),
  workspace: uuid(),
  project: uuid(),
  name: z.string(),
  description: z.string(),
  description_text: z.nullable(z.unknown()),
  description_html: z.nullable(z.unknown()),
  start_date: nullableDate(),
  target_date: nullableDate(),
  status: z.enum(MODULE_STATUSES),
  lead: nullableUuid(),
  lead_id: nullableUuid(),
  members: z.array(uuid()),
  member_ids: z.array(uuid()),
  view_props: z.record(z.string(), z.unknown()),
  logo_props: z.record(z.string(), z.unknown()),
  sort_order: z.number(),
  external_source: z.nullable(z.string()),
  external_id: z.nullable(z.string()),
  created_at: dateTime(),
  updated_at: dateTime(),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
  deleted_at: nullableDateTime(),
  archived_at: nullableDateTime(),
});
export type ModuleWriteEcho = z.infer<typeof ModuleWriteEcho>;

/**
 * Archive/restore refusal body. The server's explicit check names the rule
 * it enforced; the UI shows that sentence and never reports success.
 */
export const ModuleArchiveError = z.object({
  error: z.string(),
});
export type ModuleArchiveError = z.infer<typeof ModuleArchiveError>;

/**
 * Refusal body shared by archive/restore and the module writes. An explicit
 * check answers `{"error": "<sentence>"}` while serializer validation
 * answers a field map (`{"name": ["..."]}`); the duplicate-name refusal can
 * arrive under `error` as a list. Readers branch on the message, never on
 * success.
 */
const messageList = z.array(z.string());
export const ModuleErrorBody = z.union([
  z.object({ error: z.union([z.string(), messageList]) }),
  z
    .record(z.string(), messageList)
    .check(z.refine((fields) => Object.keys(fields).length > 0, "a refusal names at least one field")),
]);
export type ModuleErrorBody = z.infer<typeof ModuleErrorBody>;

// ---------------------------------------------------------------- inputs

/** Fields a create may set. Only the name is required. */
export interface ModuleCreateInput {
  name: string;
  description?: string;
  start_date?: string | null;
  target_date?: string | null;
  status?: ModuleStatus;
  lead_id?: string | null;
  member_ids?: string[];
}

/**
 * Thin workspace module row for cross-project hosts (the workspace
 * view dialog's filter builder). The workspace list is the full
 * module serializer with the project list's favorite annotation
 * missing, so `is_favorite` stays optional and the schema ignores
 * the serializer's remaining keys.
 */
export const WorkspaceModuleRow = z.object({
  id: uuid(),
  project_id: uuid(),
  name: z.string(),
  status: z.string(),
  is_favorite: z.optional(z.boolean()),
});
export type WorkspaceModuleRow = z.infer<typeof WorkspaceModuleRow>;

export const WorkspaceModuleList = z.array(WorkspaceModuleRow);
export type WorkspaceModuleList = z.infer<typeof WorkspaceModuleList>;

/** GET WS/modules/ — workspace-wide rows for switchers and pickers. */
export async function listWorkspaceModules(client: ApiClient, slug: string): Promise<WorkspaceModuleList> {
  const path = workspaceModulesPath(slug);
  const body = await client.getJson<unknown>(path);
  return client.parse(WorkspaceModuleList, body, `GET ${path}`);
}

/** Fields a partial update may change. */
export interface ModulePatch {
  name?: string;
  description?: string;
  start_date?: string | null;
  target_date?: string | null;
  status?: ModuleStatus;
  lead_id?: string | null;
  member_ids?: string[];
  /** Manual placement rank, written by the timeline row reorder (MOD-021). */
  sort_order?: number;
}

/**
 * Full replace body: the same fields as create, all of them present. The
 * name must be supplied; date fields are explicit nulls when cleared.
 */
export type ModuleReplaceInput = ModuleCreateInput;

/** Link write body. The server validates the URL and defaults its scheme. */
export interface ModuleLinkInput {
  url: string;
  title?: string | null;
}

/** Favorites write body names the module under `module`. */
export const ModuleFavoriteInput = z.object({
  module: uuid(),
});
export type ModuleFavoriteInput = z.infer<typeof ModuleFavoriteInput>;

/**
 * Body of the work-item-side membership write (`POST
 * .../issues/{issueId}/modules/`): the modules to add, plus the ones to
 * drop in the same call. The Issues contract's `setIssueModules` posts it
 * and stays the only writer.
 */
export const ModuleMembershipInput = z.object({
  modules: z.array(uuid()),
  removed_modules: z.optional(z.array(uuid())),
});
export type ModuleMembershipInput = z.infer<typeof ModuleMembershipInput>;

// ---------------------------------------------------------------- reads

/**
 * Thin project module list for the shell palette and the shared picker: the
 * five fields those callers read. Module surfaces use
 * `listProjectModules` for the full rows.
 */
export async function listModules(client: ApiClient, slug: string, projectId: string): Promise<ModuleList> {
  const path = modulesCollection(slug, projectId);
  const body = await client.getJson<unknown>(path);
  return client.parse(ModuleList, body, `GET ${path}`);
}

/**
 * Project modules with the full row the modules list, board and timeline
 * render: counts, dates, lead, members and the favorite flag. Archived
 * modules are excluded by the server.
 */
export async function listProjectModules(client: ApiClient, slug: string, projectId: string): Promise<ModuleRowList> {
  const path = modulesCollection(slug, projectId);
  const body = await client.getJson<unknown>(path);
  return client.parse(ModuleRowList, body, `GET ${path}`);
}

/** Detail for the module page and the peek panel. */
export async function getModule(
  client: ApiClient,
  slug: string,
  projectId: string,
  moduleId: string
): Promise<ModuleDetail> {
  const path = moduleDetailPath(slug, projectId, moduleId);
  const body = await client.getJson<unknown>(path);
  return client.parse(ModuleDetail, body, `GET ${path}`);
}

/** Links attached to a module; the sidebar lists them newest first. */
export async function listModuleLinks(
  client: ApiClient,
  slug: string,
  projectId: string,
  moduleId: string
): Promise<ModuleLinkList> {
  const path = moduleLinksPath(slug, projectId, moduleId);
  const body = await client.getJson<unknown>(path);
  return client.parse(ModuleLinkList, body, `GET ${path}`);
}

// ---------------------------------------------------------------- writes

/** Create a module. Answers 201 with the created row. */
export async function createModule(
  client: ApiClient,
  slug: string,
  projectId: string,
  input: ModuleCreateInput
): Promise<ModuleRow> {
  const path = modulesCollection(slug, projectId);
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(ModuleRow, envelope.data, `POST ${path}`);
}

/** Patch the fields the user changed. Answers 200 with the updated row. */
export async function updateModule(
  client: ApiClient,
  slug: string,
  projectId: string,
  moduleId: string,
  input: ModulePatch
): Promise<ModuleUpdate> {
  const path = moduleDetailPath(slug, projectId, moduleId);
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: input });
  return client.parse(ModuleUpdate, envelope.data, `PATCH ${path}`);
}

/**
 * Full replace for the edit form's submit. Answers 200 with the write echo,
 * not the annotated read row, so callers re-read for counts and favorites.
 */
export async function replaceModule(
  client: ApiClient,
  slug: string,
  projectId: string,
  moduleId: string,
  input: ModuleReplaceInput
): Promise<ModuleWriteEcho> {
  const path = moduleDetailPath(slug, projectId, moduleId);
  const envelope = await client.sendJson<unknown>("PUT", path, { json: input });
  return client.parse(ModuleWriteEcho, envelope.data, `PUT ${path}`);
}

/** Delete a module. Answers 204; the server 403s a non-creator member. */
export async function deleteModule(
  client: ApiClient,
  slug: string,
  projectId: string,
  moduleId: string
): Promise<void> {
  await client.delete(moduleDetailPath(slug, projectId, moduleId));
}

/** Detach one work item from a module. Answers 204. */
export async function removeIssueFromModule(
  client: ApiClient,
  slug: string,
  projectId: string,
  moduleId: string,
  issueId: string
): Promise<void> {
  await client.delete(moduleIssuePath(slug, projectId, moduleId, issueId));
}

/** Create a link. Answers 201 with the link; a duplicate URL is refused. */
export async function createModuleLink(
  client: ApiClient,
  slug: string,
  projectId: string,
  moduleId: string,
  input: ModuleLinkInput
): Promise<ModuleLink> {
  const path = moduleLinksPath(slug, projectId, moduleId);
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(ModuleLink, envelope.data, `POST ${path}`);
}

/**
 * Update a link. The server validates `url` unconditionally, so the body
 * always carries it even when only the title changed. Answers 200.
 */
export async function updateModuleLink(
  client: ApiClient,
  slug: string,
  projectId: string,
  moduleId: string,
  linkId: string,
  input: ModuleLinkInput
): Promise<ModuleLink> {
  const path = moduleLinkPath(slug, projectId, moduleId, linkId);
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: input });
  return client.parse(ModuleLink, envelope.data, `PATCH ${path}`);
}

/** Delete a link. Answers 204. */
export async function deleteModuleLink(
  client: ApiClient,
  slug: string,
  projectId: string,
  moduleId: string,
  linkId: string
): Promise<void> {
  await client.delete(moduleLinkPath(slug, projectId, moduleId, linkId));
}

/** Favorite a module for the signed-in user. Answers 204. */
export async function favoriteModule(
  client: ApiClient,
  slug: string,
  projectId: string,
  moduleId: string
): Promise<void> {
  await client.post(moduleFavoritesPath(slug, projectId), { json: { module: moduleId } });
}

/** Remove the signed-in user's favorite. Answers 204. */
export async function unfavoriteModule(
  client: ApiClient,
  slug: string,
  projectId: string,
  moduleId: string
): Promise<void> {
  await client.delete(moduleFavoritePath(slug, projectId, moduleId));
}

/**
 * One row of the archived-modules list (ARCH-020). A flat array, newest
 * created first within favorites. Module dates are calendar dates (the
 * model stores DateFields, unlike cycles); the rich-text description
 * columns carry editor JSON or null. The backend selects its columns
 * with .values(), so every key below is always present.
 */
export const ArchivedModuleListItem = z.object({
  id: uuid(),
  workspace_id: uuid(),
  project_id: uuid(),
  name: z.string(),
  description: z.string(),
  description_text: z.nullable(z.unknown()),
  description_html: z.nullable(z.unknown()),
  start_date: nullableDate(),
  target_date: nullableDate(),
  status: z.string(),
  lead_id: nullableUuid(),
  member_ids: z.array(uuid()),
  total_issues: z.number(),
  is_favorite: z.boolean(),
  cancelled_issues: z.number(),
  completed_issues: z.number(),
  started_issues: z.number(),
  unstarted_issues: z.number(),
  backlog_issues: z.number(),
  created_at: dateTime(),
  updated_at: dateTime(),
  archived_at: dateTime(),
});
export type ArchivedModuleListItem = z.infer<typeof ArchivedModuleListItem>;

export const ArchivedModuleList = z.array(ArchivedModuleListItem);
export type ArchivedModuleList = z.infer<typeof ArchivedModuleList>;

/** GET .../archived-modules/: the project's archived modules, flat. */
export async function listArchivedModules(
  client: ApiClient,
  slug: string,
  projectId: string
): Promise<ArchivedModuleList> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/projects/${projectId}/archived-modules/`);
  return client.parse(ArchivedModuleList, body, `GET archived modules ${projectId}`);
}

/** One read-only link row on the archived module detail. */
export const ArchivedModuleLink = z.object({
  id: uuid(),
  title: z.nullable(z.string()),
  url: z.string(),
});
export type ArchivedModuleLink = z.infer<typeof ArchivedModuleLink>;

/**
 * Archived module detail for the peek body (ARCH-020): the list columns
 * plus links, the sub-issue count, the per-group estimate sums and the
 * analytics summaries. The backend's extra model columns (view_props,
 * sort_order, external ids, logo) are ignored — the peek never reads
 * them.
 */
export const ArchivedModuleDetail = z.object({
  ...ArchivedModuleListItem.shape,
  link_module: z.array(ArchivedModuleLink),
  sub_issues: z.nullable(z.number()),
  total_estimate_points: z.number(),
  completed_estimate_points: z.number(),
  backlog_estimate_points: z.number(),
  unstarted_estimate_points: z.number(),
  started_estimate_points: z.number(),
  cancelled_estimate_points: z.number(),
  estimate_distribution: ArchivedEstimateDistribution,
  distribution: ArchivedDistribution,
});
export type ArchivedModuleDetail = z.infer<typeof ArchivedModuleDetail>;

/** GET .../archived-modules/<id>/: one archived module for the peek. */
export async function getArchivedModule(
  client: ApiClient,
  slug: string,
  projectId: string,
  moduleId: string
): Promise<ArchivedModuleDetail> {
  const body = await client.getJson<unknown>(
    `/api/workspaces/${slug}/projects/${projectId}/archived-modules/${moduleId}/`
  );
  return client.parse(ArchivedModuleDetail, body, `GET archived module ${moduleId}`);
}

/**
 * Module archive stamp: POST .../modules/<id>/archive/ answers 200 with
 * the stamp. Only finished modules archive (status completed or
 * cancelled, otherwise 400), so callers gate the entry on the status.
 * The backend str()s a datetime (space-separated, not ISO), so the
 * stamp stays a plain string like the project one.
 */
export const ModuleArchiveStamp = z.object({
  archived_at: z.string(),
});
export type ModuleArchiveStamp = z.infer<typeof ModuleArchiveStamp>;

export async function archiveModule(
  client: ApiClient,
  slug: string,
  projectId: string,
  moduleId: string
): Promise<ModuleArchiveStamp> {
  const path = moduleArchivePath(slug, projectId, moduleId);
  const envelope = await client.sendJson<unknown>("POST", path, {});
  return client.parse(ModuleArchiveStamp, envelope.data, `POST ${path}`);
}

/**
 * DELETE .../modules/<id>/archive/ clears the stamp and restores the
 * module. Resolves the status (204); the archived-modules unarchive
 * alias 404s and is deliberately not contracted.
 */
export async function restoreModule(
  client: ApiClient,
  slug: string,
  projectId: string,
  moduleId: string
): Promise<number> {
  const response = await client.delete(moduleArchivePath(slug, projectId, moduleId));
  return response.status;
}
