// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public board display data (parity/specs/space.md §5.1): the workflow
// states, labels, members, cycles and modules the board maps onto its
// rows, filter pills and pickers. All five are public, anchor-scoped and
// answer 404 for an unpublished anchor. The board itself only fetches
// states, labels and members; cycles and modules exist for parity with
// the API surface and for future rows.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { nullableUuid, uuid } from "./fields.js";
import { SPACE_ISSUE_STATE_GROUPS } from "./space-issues.js";

/** A workflow state as the public board receives it. */
export const SpaceBoardState = z.object({
  id: uuid(),
  name: z.string(),
  color: z.string(),
  group: z.enum(SPACE_ISSUE_STATE_GROUPS),
  sequence: z.number(),
});
export type SpaceBoardState = z.infer<typeof SpaceBoardState>;

export const SpaceBoardLabel = z.object({
  id: uuid(),
  name: z.string(),
  color: z.string(),
  parent: nullableUuid(),
});
export type SpaceBoardLabel = z.infer<typeof SpaceBoardLabel>;

/**
 * A public board member. The endpoint answers the `values()` projection:
 * the member and avatar ride under their column names, not a nested user.
 */
export const SpaceBoardMember = z.object({
  id: uuid(),
  member: uuid(),
  member__display_name: z.string(),
  member__avatar: z.string(),
});
export type SpaceBoardMember = z.infer<typeof SpaceBoardMember>;

/** Minimal cycle projection the public endpoint answers. */
export const SpaceBoardCycle = z.object({
  id: uuid(),
  name: z.string(),
});
export type SpaceBoardCycle = z.infer<typeof SpaceBoardCycle>;

/** Minimal module projection the public endpoint answers. */
export const SpaceBoardModule = z.object({
  id: uuid(),
  name: z.string(),
});
export type SpaceBoardModule = z.infer<typeof SpaceBoardModule>;

function displayPath(anchor: string, resource: string): string {
  return `/api/public/anchor/${encodeURIComponent(anchor)}/${resource}/`;
}

export async function listSpaceBoardStates(client: ApiClient, anchor: string): Promise<SpaceBoardState[]> {
  const path = displayPath(anchor, "states");
  const body = await client.getJson<unknown>(path);
  return client.parse(z.array(SpaceBoardState), body, `GET ${path}`);
}

export async function listSpaceBoardLabels(client: ApiClient, anchor: string): Promise<SpaceBoardLabel[]> {
  const path = displayPath(anchor, "labels");
  const body = await client.getJson<unknown>(path);
  return client.parse(z.array(SpaceBoardLabel), body, `GET ${path}`);
}

export async function listSpaceBoardMembers(client: ApiClient, anchor: string): Promise<SpaceBoardMember[]> {
  const path = displayPath(anchor, "members");
  const body = await client.getJson<unknown>(path);
  return client.parse(z.array(SpaceBoardMember), body, `GET ${path}`);
}

export async function listSpaceBoardCycles(client: ApiClient, anchor: string): Promise<SpaceBoardCycle[]> {
  const path = displayPath(anchor, "cycles");
  const body = await client.getJson<unknown>(path);
  return client.parse(z.array(SpaceBoardCycle), body, `GET ${path}`);
}

export async function listSpaceBoardModules(client: ApiClient, anchor: string): Promise<SpaceBoardModule[]> {
  const path = displayPath(anchor, "modules");
  const body = await client.getJson<unknown>(path);
  return client.parse(z.array(SpaceBoardModule), body, `GET ${path}`);
}
