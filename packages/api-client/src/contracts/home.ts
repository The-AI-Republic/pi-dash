// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Home contracts: workspace dashboard, home preferences, quick links,
// recent visits and the new-user tour flag.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableDateTime, nullableUuid, uuid } from "./fields.js";

/**
 * Live home-dashboard endpoint.
 * GET /api/users/me/workspaces/<slug>/dashboard/ answers per-user issue
 * stats for the workspace: activity buckets, completion buckets, counts,
 * state distribution and dated issue rows.
 */
export const DashboardActivityBucket = z.object({
  created_date: z.string(),
  activity_count: z.number(),
});
export type DashboardActivityBucket = z.infer<typeof DashboardActivityBucket>;

export const DashboardCompletedBucket = z.object({
  week_in_month: z.number(),
  completed_count: z.number(),
});
export type DashboardCompletedBucket = z.infer<typeof DashboardCompletedBucket>;

export const DashboardStateSlice = z.object({
  state_group: z.string(),
  state_count: z.number(),
});
export type DashboardStateSlice = z.infer<typeof DashboardStateSlice>;

export const DashboardOverdueIssue = z.object({
  id: uuid(),
  name: z.string(),
  workspace__slug: z.string(),
  project_id: uuid(),
  target_date: nullableDateTime(),
});
export type DashboardOverdueIssue = z.infer<typeof DashboardOverdueIssue>;

export const DashboardUpcomingIssue = z.object({
  id: uuid(),
  name: z.string(),
  workspace__slug: z.string(),
  project_id: uuid(),
  start_date: nullableDateTime(),
});
export type DashboardUpcomingIssue = z.infer<typeof DashboardUpcomingIssue>;

export const WorkspaceDashboard = z.object({
  issue_activities: z.array(DashboardActivityBucket),
  completed_issues: z.array(DashboardCompletedBucket),
  assigned_issues_count: z.number(),
  pending_issues_count: z.number(),
  completed_issues_count: z.number(),
  issues_due_week_count: z.number(),
  state_distribution: z.array(DashboardStateSlice),
  overdue_issues: z.array(DashboardOverdueIssue),
  upcoming_issues: z.array(DashboardUpcomingIssue),
});
export type WorkspaceDashboard = z.infer<typeof WorkspaceDashboard>;

/**
 * The `month` query param selects which month's completions are bucketed
 * (the server defaults it to 1 when absent).
 */
export async function getWorkspaceDashboard(
  client: ApiClient,
  slug: string,
  month?: number
): Promise<WorkspaceDashboard> {
  const path = `/api/users/me/workspaces/${slug}/dashboard/`;
  const body = await client.getJson<unknown>(path, month === undefined ? {} : { query: { month } });
  return client.parse(WorkspaceDashboard, body, `GET ${path}`);
}

export const HOME_WIDGET_KEYS = ["quick_links", "recents", "my_stickies", "new_at_pi_dash", "quick_tutorial"] as const;
export type HomeWidgetKey = (typeof HOME_WIDGET_KEYS)[number];

/**
 * Home preference row. GET /api/workspaces/<slug>/home-preferences/
 * answers the plain `.values()` projection (config included); the per-key
 * PATCH answers the narrower serializer shape without `config`.
 */
export const HomePreference = z.object({
  key: z.enum(HOME_WIDGET_KEYS),
  is_enabled: z.boolean(),
  config: z.record(z.string(), z.unknown()),
  sort_order: z.number(),
});
export type HomePreference = z.infer<typeof HomePreference>;

export const HomePreferenceList = z.array(HomePreference);
export type HomePreferenceList = z.infer<typeof HomePreferenceList>;

export async function listHomePreferences(client: ApiClient, slug: string): Promise<HomePreferenceList> {
  const path = `/api/workspaces/${slug}/home-preferences/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(HomePreferenceList, body, `GET ${path}`);
}

export interface HomePreferencePatch {
  is_enabled?: boolean;
  sort_order?: number;
  config?: Record<string, unknown>;
}

export const HomePreferenceUpdate = z.object({
  key: z.enum(HOME_WIDGET_KEYS),
  is_enabled: z.boolean(),
  sort_order: z.number(),
});
export type HomePreferenceUpdate = z.infer<typeof HomePreferenceUpdate>;

export async function updateHomePreference(
  client: ApiClient,
  slug: string,
  key: HomeWidgetKey,
  patch: HomePreferencePatch
): Promise<HomePreferenceUpdate> {
  const path = `/api/workspaces/${slug}/home-preferences/${key}/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: patch });
  return client.parse(HomePreferenceUpdate, envelope.data, `PATCH ${path}`);
}

/**
 * Quick link row (WorkspaceUserLink, every field). The server prefixes a
 * bare `url` with `http://` on write and validates it as a URL.
 */
export const QuickLink = z.object({
  id: uuid(),
  workspace: uuid(),
  owner: uuid(),
  title: z.nullable(z.string()),
  url: z.string(),
  metadata: z.record(z.string(), z.unknown()),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
  created_at: dateTime(),
  updated_at: dateTime(),
});
export type QuickLink = z.infer<typeof QuickLink>;

export const QuickLinkList = z.array(QuickLink);
export type QuickLinkList = z.infer<typeof QuickLinkList>;

export async function listQuickLinks(client: ApiClient, slug: string): Promise<QuickLinkList> {
  const path = `/api/workspaces/${slug}/quick-links/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(QuickLinkList, body, `GET ${path}`);
}

export interface QuickLinkInput {
  title?: string;
  url: string;
}

export async function createQuickLink(client: ApiClient, slug: string, input: QuickLinkInput): Promise<QuickLink> {
  const path = `/api/workspaces/${slug}/quick-links/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(QuickLink, envelope.data, `POST ${path}`);
}

export async function getQuickLink(client: ApiClient, slug: string, id: string): Promise<QuickLink> {
  const path = `/api/workspaces/${slug}/quick-links/${id}/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(QuickLink, body, `GET ${path}`);
}

export async function updateQuickLink(
  client: ApiClient,
  slug: string,
  id: string,
  patch: Partial<QuickLinkInput>
): Promise<QuickLink> {
  const path = `/api/workspaces/${slug}/quick-links/${id}/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: patch });
  return client.parse(QuickLink, envelope.data, `PATCH ${path}`);
}

/** DELETE answers 204 with no body; resolves the response status. */
export async function deleteQuickLink(client: ApiClient, slug: string, id: string): Promise<number> {
  const response = await client.delete(`/api/workspaces/${slug}/quick-links/${id}/`);
  return response.status;
}

/**
 * Recent visit row. `entity_data` is the visited entity's own serialized
 * shape (issue, page or project), or null when it is gone — callers narrow
 * it per `entity_name`.
 */
export const RecentVisit = z.object({
  id: uuid(),
  entity_name: z.string(),
  entity_identifier: z.string(),
  entity_data: z.nullable(z.record(z.string(), z.unknown())),
  visited_at: dateTime(),
});
export type RecentVisit = z.infer<typeof RecentVisit>;

export const RecentVisitList = z.array(RecentVisit);
export type RecentVisitList = z.infer<typeof RecentVisitList>;

/** Optional `entityName` narrows to one entity kind (issue, page, project). */
export async function listRecentVisits(client: ApiClient, slug: string, entityName?: string): Promise<RecentVisitList> {
  const path = `/api/workspaces/${slug}/recent-visits/`;
  const body = await client.getJson<unknown>(
    path,
    entityName === undefined ? {} : { query: { entity_name: entityName } }
  );
  return client.parse(RecentVisitList, body, `GET ${path}`);
}
