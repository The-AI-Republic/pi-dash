// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Single-issue actions: the full update and create plus the thin delete,
// cycle and module membership, and subscriptions. The list and detail
// reads stay in issues.ts; everything here runs from lazy surfaces (the
// palette's contextual edits, the detail page, the layouts), so it stays
// out of the initial bundle (NEWFRONT-523, size gate).
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { nullableDateTime, uuid } from "./fields.js";
import { ISSUE_PRIORITIES, IssueListItem } from "./issues.js";
import type { ModuleMembershipInput } from "./modules.js";

/**
 * Full work-item update (issues area): every field the layouts, modal and
 * detail write through PATCH .../issues/<id>/. Cycle and module
 * membership are NOT here — they go through the bridge calls below.
 * `git_work_branch` is write-only: no read returns it (backend pin
 * NEWFRONT-140), so callers treat the 204 as the only confirmation.
 * The endpoint answers 204 with no body, so callers re-read through
 * getIssue to prove the write.
 */
export interface IssuePatch {
  name?: string;
  description_html?: string;
  state_id?: string | null;
  priority?: (typeof ISSUE_PRIORITIES)[number];
  assignee_ids?: string[];
  label_ids?: string[];
  estimate_point?: string | null;
  /** Execution-target patch (ISS-218): pod pin plus executor override, each nullable. */
  assigned_pod_id?: string | null;
  /**
   * Dedicated-runner pin (hybrid assignment, design §14.1): the runner's
   * pod must travel in assigned_pod_id and the executor must be
   * local_runner in the same write; null clears back to the pod.
   */
  assigned_runner_id?: string | null;
  agent_executor?: string | null;
  start_date?: string | null;
  target_date?: string | null;
  parent_id?: string | null;
  sort_order?: number;
  git_work_branch?: string;
}

/**
 * Work-item update: the palette's contextual edits plus every issues-area
 * inline edit PATCH here. Answers 204 with no body.
 */
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

/**
 * Full work-item create (issues area): the create modal, quick-add and
 * calendar add post here. Sub-issue create shares this endpoint with
 * `parent_id` set (NEWFRONT-612 consumes it). Cycle and module attach
 * are separate bridge calls after the create; `sort_order` is
 * server-assigned (largest + step) and any sent value is advisory.
 */
export interface IssueCreateInput {
  name: string;
  description_html?: string;
  state_id?: string | null;
  priority?: (typeof ISSUE_PRIORITIES)[number];
  assignee_ids?: string[];
  label_ids?: string[];
  estimate_point?: string | null;
  start_date?: string | null;
  target_date?: string | null;
  parent_id?: string | null;
  git_work_branch?: string;
  /**
   * Execution target picked in the create dialog (ISS-117/ISS-218): a
   * pod pin plus the executor, or an executor override with a null pod.
   * The serializer accepts both on create; absent means the project
   * default.
   */
  assigned_pod_id?: string | null;
  agent_executor?: string | null;
  /**
   * Dedicated-runner pin picked in the create dialog (hybrid assignment,
   * design §14.1): same triple as the patch; absent means the project
   * default pod with no pin.
   */
  assigned_runner_id?: string | null;
}

/**
 * The created row: the list projection plus the deletion marker. The
 * endpoint answers 201 with the same annotated `.values()` shape the
 * list reads.
 */
export const IssueCreateResult = z.object({
  ...IssueListItem.shape,
  deleted_at: nullableDateTime(),
});
export type IssueCreateResult = z.infer<typeof IssueCreateResult>;

/**
 * Work-item create: the sidebar quick-create entry (SHELL-047) and every
 * issues-area creation surface post here. Only workspace admins and
 * members may create (guests get 403), so callers disable the entries
 * for them instead of surfacing the denial.
 */
export async function createIssue(
  client: ApiClient,
  slug: string,
  projectId: string,
  input: IssueCreateInput
): Promise<IssueCreateResult> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/issues/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(IssueCreateResult, envelope.data, `POST ${path}`);
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
  input: ModuleMembershipInput
): Promise<MembershipWriteResult> {
  const envelope = await client.sendJson<unknown>(
    "POST",
    `/api/workspaces/${slug}/projects/${projectId}/issues/${issueId}/modules/`,
    { json: input }
  );
  return client.parse(MembershipWriteResult, envelope.data, `POST issue ${issueId} modules`);
}

/**
 * Bulk module attach for a group-header "add existing" (ISS-012): one
 * call adds every selected issue to the module. Answers 201 {message}.
 * The per-issue variant (setIssueModules) stays the property-strip
 * writer; this is the module collection's own bulk bridge.
 */
export async function addIssuesToModule(
  client: ApiClient,
  slug: string,
  projectId: string,
  moduleId: string,
  issueIds: string[]
): Promise<MembershipWriteResult> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/modules/${moduleId}/issues/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: { issues: issueIds } });
  return client.parse(MembershipWriteResult, envelope.data, `POST ${path}`);
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
