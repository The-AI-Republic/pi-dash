// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace invitation and join-request contracts: the pending invites the
// inbox lists, the emailed single-invitation link, and the request-to-join
// flow for users who arrive with no workspace.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { nullableDateTime, nullableUuid, uuid } from "./fields.js";
import { WorkspaceLite } from "./workspaces.js";

/**
 * One workspace invitation row for the caller's address. Answered invites
 * stay readable with responded_at set; accepted invites on a matching
 * account are deleted instead, so re-fetching them 404s.
 */
export const WorkspaceInvitation = z.object({
  id: uuid(),
  workspace: WorkspaceLite,
  email: z.string(),
  accepted: z.boolean(),
  token: z.string(),
  message: z.nullable(z.string()),
  responded_at: nullableDateTime(),
  role: z.number(),
  invite_link: z.string(),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
  created_at: z.iso.datetime(),
  updated_at: z.iso.datetime(),
});
export type WorkspaceInvitation = z.infer<typeof WorkspaceInvitation>;

export const WorkspaceInvitationList = z.array(WorkspaceInvitation);
export type WorkspaceInvitationList = z.infer<typeof WorkspaceInvitationList>;

/** GET /api/users/me/workspaces/invitations/ — my pending invites. */
export async function listWorkspaceInvitations(client: ApiClient): Promise<WorkspaceInvitationList> {
  const path = "/api/users/me/workspaces/invitations/";
  const body = await client.getJson<unknown>(path);
  return client.parse(WorkspaceInvitationList, body, `GET ${path}`);
}

/**
 * POST /api/users/me/workspaces/invitations/ with `{ invitations }` joins
 * every listed workspace at once (204); the joined invites are deleted.
 */
export async function acceptWorkspaceInvitations(client: ApiClient, invitationIds: string[]): Promise<void> {
  await client.post("/api/users/me/workspaces/invitations/", { json: { invitations: invitationIds } });
}

/** GET /api/workspaces/<slug>/invitations/<id>/join/ — one emailed invite. */
export async function getWorkspaceInvitation(
  client: ApiClient,
  slug: string,
  invitationId: string
): Promise<WorkspaceInvitation> {
  const path = `/api/workspaces/${slug}/invitations/${invitationId}/join/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(WorkspaceInvitation, body, `GET ${path}`);
}

export const InvitationAnswer = z.object({
  message: z.string(),
});
export type InvitationAnswer = z.infer<typeof InvitationAnswer>;

export interface InvitationAnswerInput {
  accepted: boolean;
  token: string;
}

/**
 * POST .../invitations/<id>/join/ with `{ accepted, token }` answers one
 * invite. Accepting with a matching account deletes the row; declining
 * keeps it with responded_at set. A wrong token answers 403.
 */
export async function answerWorkspaceInvitation(
  client: ApiClient,
  slug: string,
  invitationId: string,
  input: InvitationAnswerInput
): Promise<InvitationAnswer> {
  const path = `/api/workspaces/${slug}/invitations/${invitationId}/join/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(InvitationAnswer, envelope.data, `POST ${path}`);
}

export interface WorkspaceInvitee {
  email: string;
  role?: number;
}

export interface WorkspaceInviteInput {
  emails: WorkspaceInvitee[];
}

export const BulkInviteResult = z.object({
  message: z.string(),
});
export type BulkInviteResult = z.infer<typeof BulkInviteResult>;

/**
 * POST /api/workspaces/<slug>/invitations/ with `{ emails }` invites a
 * batch of addresses (200 + message). Unknown roles default to guest;
 * inviting an existing member answers 400 naming the members.
 */
export async function sendWorkspaceInvitations(
  client: ApiClient,
  slug: string,
  input: WorkspaceInviteInput
): Promise<BulkInviteResult> {
  const path = `/api/workspaces/${slug}/invitations/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(BulkInviteResult, envelope.data, `POST ${path}`);
}

/**
 * The join-request requester. Shaped like UserLite except avatar_url,
 * which is null for users who never set an avatar.
 */
export const JoinRequestRequester = z.object({
  id: uuid(),
  first_name: z.string(),
  last_name: z.string(),
  avatar: z.string(),
  avatar_url: z.nullable(z.string()),
  display_name: z.string(),
  is_bot: z.boolean(),
});
export type JoinRequestRequester = z.infer<typeof JoinRequestRequester>;

/**
 * One request-to-join row. The requester-facing list deliberately omits
 * the resolved workspace, so the pending state looks identical whether or
 * not the typed admin address belonged to a real admin.
 */
export const WorkspaceJoinRequest = z.object({
  id: uuid(),
  requester: JoinRequestRequester,
  admin_email: z.string(),
  message: z.nullable(z.string()),
  status: z.enum(["PENDING", "APPROVED", "DENIED"]),
  responded_at: nullableDateTime(),
  created_at: z.iso.datetime(),
  updated_at: z.iso.datetime(),
});
export type WorkspaceJoinRequest = z.infer<typeof WorkspaceJoinRequest>;

export const WorkspaceJoinRequestList = z.array(WorkspaceJoinRequest);
export type WorkspaceJoinRequestList = z.infer<typeof WorkspaceJoinRequestList>;

/** GET /api/users/me/workspaces/join-requests/ — my own requests. */
export async function listWorkspaceJoinRequests(client: ApiClient): Promise<WorkspaceJoinRequestList> {
  const path = "/api/users/me/workspaces/join-requests/";
  const body = await client.getJson<unknown>(path);
  return client.parse(WorkspaceJoinRequestList, body, `GET ${path}`);
}

export interface JoinRequestInput {
  admin_email: string;
  message?: string;
}

export const JoinRequestResult = z.object({
  message: z.string(),
  workspace_slug: z.optional(z.string()),
});
export type JoinRequestResult = z.infer<typeof JoinRequestResult>;

/**
 * POST /api/users/me/workspaces/join-requests/ with `{ admin_email }`
 * asks to join (201 + message either way, so the call reveals nothing
 * about the address). When the address administers only workspaces the
 * caller already belongs to, the call instead names one (200).
 */
export async function createWorkspaceJoinRequest(
  client: ApiClient,
  input: JoinRequestInput
): Promise<JoinRequestResult> {
  const path = "/api/users/me/workspaces/join-requests/";
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(JoinRequestResult, envelope.data, `POST ${path}`);
}
