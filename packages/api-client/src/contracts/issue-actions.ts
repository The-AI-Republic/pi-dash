// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Single-issue actions: the thin update and delete plus cycle and module
// membership and subscriptions. The list and detail reads stay in
// issues.ts; everything here runs from lazy surfaces (the palette's
// contextual edits, the detail page), so it stays out of the initial
// bundle (NEWFRONT-523, size gate).
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { uuid } from "./fields.js";
import { ISSUE_PRIORITIES } from "./issues.js";

/**
 * Thin work-item update for the shell: the command palette's contextual
 * edits (rename, state, priority, assignees, labels, estimate,
 * description). The endpoint answers 204 with no body, so callers re-read
 * through getIssue above. Full issue ownership stays in the issues area.
 */
export interface IssuePatch {
  name?: string;
  description_html?: string;
  state_id?: string | null;
  priority?: (typeof ISSUE_PRIORITIES)[number];
  assignee_ids?: string[];
  label_ids?: string[];
  estimate_point?: string | null;
}

export async function updateIssue(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string,
  input: IssuePatch
): Promise<void> {
  await client.patch(`/api/workspaces/${slug}/projects/${projectId}/issues/${issueId}/`, { json: input });
}

/**
 * Thin work-item delete for the shell: the palette's Delete entry
 * confirms, then removes the item. The endpoint answers 204; only a
 * project admin or the item's creator may delete (otherwise 403), so
 * callers surface the failure instead of navigating away. Full issue
 * ownership stays in the issues area.
 */
export async function deleteIssue(client: ApiClient, slug: string, projectId: string, issueId: string): Promise<void> {
  await client.delete(`/api/workspaces/${slug}/projects/${projectId}/issues/${issueId}/`);
}

/** 201 {message} envelope shared by the membership writes below. */
export const MembershipWriteResult = z.object({
  message: z.string(),
});
export type MembershipWriteResult = z.infer<typeof MembershipWriteResult>;

/**
 * Thin cycle membership for the shell: the palette's Add-to-cycle picker
 * moves the item with one call (the server re-points an existing bridge).
 * Answers 201 {message}. Full cycle ownership stays in the cycles area.
 */
export async function addIssueToCycle(
  client: ApiClient,
  slug: string,
  projectId: string,
  cycleId: string,
  issueId: string
): Promise<MembershipWriteResult> {
  const envelope = await client.sendJson<unknown>(
    "POST",
    `/api/workspaces/${slug}/projects/${projectId}/cycles/${cycleId}/cycle-issues/`,
    { json: { issues: [issueId] } }
  );
  return client.parse(MembershipWriteResult, envelope.data, `POST cycle ${cycleId} issues`);
}

/**
 * Thin cycle removal for the shell. The bridge position takes the issue
 * id (the URL names it issue_id), so no bridge lookup is needed.
 * Answers 204. Full cycle ownership stays in the cycles area.
 */
export async function removeIssueFromCycle(
  client: ApiClient,
  slug: string,
  projectId: string,
  cycleId: string,
  issueId: string
): Promise<void> {
  await client.delete(`/api/workspaces/${slug}/projects/${projectId}/cycles/${cycleId}/cycle-issues/${issueId}/`);
}

/**
 * Thin module membership for the shell: the palette's Add-to-modules
 * picker adds and removes in one call. Answers 201 {message}. Full
 * module ownership stays in the modules area.
 */
export async function setIssueModules(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string,
  input: { modules: string[]; removed_modules?: string[] }
): Promise<MembershipWriteResult> {
  const envelope = await client.sendJson<unknown>(
    "POST",
    `/api/workspaces/${slug}/projects/${projectId}/issues/${issueId}/modules/`,
    { json: input }
  );
  return client.parse(MembershipWriteResult, envelope.data, `POST issue ${issueId} modules`);
}

export const SubscriptionStatus = z.object({
  subscribed: z.boolean(),
});
export type SubscriptionStatus = z.infer<typeof SubscriptionStatus>;

export async function getSubscriptionStatus(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string
): Promise<SubscriptionStatus> {
  const body = await client.getJson<unknown>(
    `/api/workspaces/${slug}/projects/${projectId}/issues/${issueId}/subscribe/`
  );
  return client.parse(SubscriptionStatus, body, `GET issue subscribe status ${issueId}`);
}

export const IssueSubscription = z.object({
  id: uuid(),
  subscriber: uuid(),
});
export type IssueSubscription = z.infer<typeof IssueSubscription>;

export async function subscribeIssue(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string
): Promise<IssueSubscription> {
  const envelope = await client.sendJson<unknown>(
    "POST",
    `/api/workspaces/${slug}/projects/${projectId}/issues/${issueId}/subscribe/`,
    {}
  );
  return client.parse(IssueSubscription, envelope.data, `POST issue subscribe ${issueId}`);
}

export async function unsubscribeIssue(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string
): Promise<void> {
  await client.delete(`/api/workspaces/${slug}/projects/${projectId}/issues/${issueId}/subscribe/`);
}
