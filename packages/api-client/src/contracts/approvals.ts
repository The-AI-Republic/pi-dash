// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Approval contracts: the pending queue behind the approvals tab and
// the per-card decision. Deciding resumes the owning run; a decision
// is recorded with its web source and decider.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableDateTime, uuid } from "./fields.js";

export const APPROVAL_KINDS = ["command_execution", "file_change", "network_access", "other"] as const;
export type ApprovalKind = (typeof APPROVAL_KINDS)[number];

export const APPROVAL_STATUSES = ["pending", "accepted", "declined", "expired"] as const;
export type ApprovalStatus = (typeof APPROVAL_STATUSES)[number];

export const APPROVAL_DECISIONS = ["accept", "decline", "accept_for_session"] as const;
export type ApprovalDecision = (typeof APPROVAL_DECISIONS)[number];

/**
 * One approval request. Decisions and expiry move `status` off
 * pending; the queue lists pending rows only.
 */
export const ApprovalRequest = z.object({
  id: uuid(),
  agent_run: uuid(),
  kind: z.enum(APPROVAL_KINDS),
  payload: z.record(z.string(), z.unknown()),
  reason: z.string(),
  status: z.enum(APPROVAL_STATUSES),
  decision_source: z.string(),
  requested_at: dateTime(),
  decided_at: nullableDateTime(),
  expires_at: nullableDateTime(),
});
export type ApprovalRequest = z.infer<typeof ApprovalRequest>;

export const ApprovalRequestList = z.array(ApprovalRequest);
export type ApprovalRequestList = z.infer<typeof ApprovalRequestList>;

/**
 * Pending approvals routed to the caller, newest first, capped at
 * 200 rows. Narrow to one project with `projectId`.
 */
export async function listApprovals(client: ApiClient, projectId?: string): Promise<ApprovalRequestList> {
  const path = "/api/runners/approvals/";
  const body = await client.getJson<unknown>(path, { query: { project: projectId } });
  return client.parse(ApprovalRequestList, body, `GET ${path}`);
}

/**
 * Decide one approval. The acted row answers with its new status;
 * the owning run resumes. An already-decided row is refused
 * with 409.
 */
export async function decideApproval(
  client: ApiClient,
  approvalId: string,
  decision: ApprovalDecision
): Promise<ApprovalRequest> {
  const path = `/api/runners/approvals/${approvalId}/decide/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: { decision } });
  return client.parse(ApprovalRequest, envelope.data, `POST ${path}`);
}
