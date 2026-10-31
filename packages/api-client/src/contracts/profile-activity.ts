// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Member-profile reads: the identity/per-project segregation, the
// assigned-work statistics, the per-user issue tabs, the paged activity
// feed with its CSV export, and the caller's own cross-workspace history.
// Display and layout choices for the issue tabs are browser-local (the
// spec's storage key), so they have no server contract here.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, uuid } from "./fields.js";
import { IssueActivity } from "./issue-activity.js";
import { GroupedIssueListResponse, IssueListResponse, issueListQueryParams } from "./issues.js";
import type { IssueListQuery } from "./issues.js";

/**
 * One project row of the segregation read: issue counts for the viewed
 * member, restricted to the projects the caller can see.
 */
export const UserProfileProject = z.object({
  id: uuid(),
  logo_props: z.record(z.string(), z.unknown()),
  created_issues: z.number(),
  assigned_issues: z.number(),
  completed_issues: z.number(),
  pending_issues: z.number(),
});
export type UserProfileProject = z.infer<typeof UserProfileProject>;

/** Identity fields the sidebar renders for the viewed member. */
export const UserProfileIdentity = z.object({
  email: z.email(),
  first_name: z.string(),
  last_name: z.string(),
  display_name: z.string(),
  avatar_url: z.nullable(z.string()),
  cover_image_url: z.nullable(z.string()),
  date_joined: dateTime(),
  user_timezone: z.string(),
});
export type UserProfileIdentity = z.infer<typeof UserProfileIdentity>;

export const UserProfileSegregation = z.object({
  user_data: UserProfileIdentity,
  project_data: z.array(UserProfileProject),
});
export type UserProfileSegregation = z.infer<typeof UserProfileSegregation>;

/** GET /api/workspaces/<slug>/user-profile/<user_id>/ */
export async function getUserProfileSegregation(
  client: ApiClient,
  slug: string,
  userId: string
): Promise<UserProfileSegregation> {
  const path = `/api/workspaces/${slug}/user-profile/${userId}/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(UserProfileSegregation, body, `GET ${path}`);
}

/** One state group's assigned-work count; groups with no rows are absent. */
export const ProfileStateBucket = z.object({
  state_group: z.string(),
  state_count: z.number(),
});
export type ProfileStateBucket = z.infer<typeof ProfileStateBucket>;

/** One priority's assigned-work count, ordered urgent first. */
export const ProfilePriorityBucket = z.object({
  priority: z.string(),
  priority_count: z.number(),
  priority_order: z.number(),
});
export type ProfilePriorityBucket = z.infer<typeof ProfilePriorityBucket>;

/** A cycle the viewed member's work sits in (current or upcoming). */
export const ProfileCycleRef = z.object({
  cycle__name: z.string(),
  cycle__id: uuid(),
  cycle__project_id: uuid(),
});
export type ProfileCycleRef = z.infer<typeof ProfileCycleRef>;

/**
 * GET /api/workspaces/<slug>/user-stats/<user_id>/ — the count cards,
 * workload row and the two distributions. Counts are always present and
 * zero for a member with no work; the distribution arrays are empty then.
 */
export const UserProfileStats = z.object({
  state_distribution: z.array(ProfileStateBucket),
  priority_distribution: z.array(ProfilePriorityBucket),
  created_issues: z.number(),
  assigned_issues: z.number(),
  completed_issues: z.number(),
  pending_issues: z.number(),
  subscribed_issues: z.number(),
  present_cycles: z.array(ProfileCycleRef),
  upcoming_cycles: z.array(ProfileCycleRef),
});
export type UserProfileStats = z.infer<typeof UserProfileStats>;

export async function getUserProfileStats(client: ApiClient, slug: string, userId: string): Promise<UserProfileStats> {
  const path = `/api/workspaces/${slug}/user-stats/${userId}/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(UserProfileStats, body, `GET ${path}`);
}

/**
 * The ownership selector for a profile issue tab: the viewed member id in
 * exactly one of the three dimensions. Layout/grouping and legacy filters
 * travel through the shared issue list query.
 */
export type ProfileOwnership = "assigned" | "created" | "subscribed";

export interface ProfileIssuesQuery extends IssueListQuery {
  ownership?: ProfileOwnership;
}

function ownershipFilter(ownership: ProfileOwnership | undefined, userId: string) {
  switch (ownership) {
    case "assigned":
      return { assignees: userId };
    case "created":
      return { created_by: userId };
    case "subscribed":
      return { subscriber: userId };
    default:
      return {};
  }
}

function profileIssuesParams(query: ProfileIssuesQuery, userId: string) {
  return issueListQueryParams({
    ...query,
    filters: { ...query.filters, ...ownershipFilter(query.ownership, userId) },
  });
}

/**
 * GET /api/workspaces/<slug>/user-issues/<user_id>/ — the same cursor
 * envelope and row projection as the project issue list, narrowed by the
 * tab's ownership selector. Grouped layouts use the grouped read; both
 * envelope types come from the issues contracts so the shapes stay one.
 */
export async function listUserProfileIssues(
  client: ApiClient,
  slug: string,
  userId: string,
  query: ProfileIssuesQuery = {}
): Promise<IssueListResponse> {
  const path = `/api/workspaces/${slug}/user-issues/${userId}/`;
  const body = await client.getJson<unknown>(path, { query: profileIssuesParams(query, userId) });
  return client.parse(IssueListResponse, body, `GET ${path}`);
}

export async function listGroupedUserProfileIssues(
  client: ApiClient,
  slug: string,
  userId: string,
  query: ProfileIssuesQuery & { groupBy: string }
): Promise<GroupedIssueListResponse> {
  const path = `/api/workspaces/${slug}/user-issues/${userId}/`;
  const body = await client.getJson<unknown>(path, { query: profileIssuesParams(query, userId) });
  return client.parse(GroupedIssueListResponse, body, `GET ${path}`);
}

/**
 * Cursor envelope shared by both activity feeds: the rows, the totals the
 * load-more control follows and the paginator's bookkeeping fields.
 */
export const UserActivityPage = z.object({
  grouped_by: z.nullable(z.string()),
  sub_grouped_by: z.nullable(z.string()),
  total_count: z.number(),
  next_cursor: z.string(),
  prev_cursor: z.string(),
  next_page_results: z.boolean(),
  prev_page_results: z.boolean(),
  count: z.number(),
  total_pages: z.number(),
  total_results: z.number(),
  extra_stats: z.nullable(z.unknown()),
  results: z.array(IssueActivity),
});
export type UserActivityPage = z.infer<typeof UserActivityPage>;

export interface ActivityQuery {
  /** Rows per page; the profile feed asks for 100. */
  perPage?: number;
  cursor?: string;
  orderBy?: string;
}

function activityParams(query: ActivityQuery) {
  return {
    per_page: query.perPage,
    cursor: query.cursor,
    order_by: query.orderBy,
  };
}

/** GET /api/workspaces/<slug>/user-activity/<user_id>/ (paged). */
export async function getUserProfileActivity(
  client: ApiClient,
  slug: string,
  userId: string,
  query: ActivityQuery = {}
): Promise<UserActivityPage> {
  const path = `/api/workspaces/${slug}/user-activity/${userId}/`;
  const body = await client.getJson<unknown>(path, { query: activityParams(query) });
  return client.parse(UserActivityPage, body, `GET ${path}`);
}

/** The exported activity file: its bytes plus the headers a download uses. */
export interface ActivityExport {
  status: number;
  contentType: string;
  filename: string;
  csv: string;
}

/**
 * POST /api/workspaces/<slug>/user-activity/<user_id>/export/ with the
 * local calendar date. Answers CSV bytes; the filename comes from the
 * server's disposition header, falling back to the documented name.
 */
export async function exportUserProfileActivity(
  client: ApiClient,
  slug: string,
  userId: string,
  date: string
): Promise<ActivityExport> {
  const path = `/api/workspaces/${slug}/user-activity/${userId}/export/`;
  const response = await client.post(path, { json: { date } });
  const disposition = response.headers.get("content-disposition") ?? "";
  const match = /filename="?([^";]+)"?/i.exec(disposition);
  return {
    status: response.status,
    contentType: response.headers.get("content-type") ?? "",
    filename: match?.[1] ?? "workspace-user-activity.csv",
    csv: await response.text(),
  };
}

/** GET /api/users/me/activities/ — the caller's history across workspaces. */
export async function listMyActivities(client: ApiClient, query: ActivityQuery = {}): Promise<UserActivityPage> {
  const body = await client.getJson<unknown>("/api/users/me/activities/", { query: activityParams(query) });
  return client.parse(UserActivityPage, body, "GET /api/users/me/activities/");
}
