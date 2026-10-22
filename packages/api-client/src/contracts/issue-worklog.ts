// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Issue worklog contracts (ISS-206, cloud leg; parity/specs/issues.md
// §16 and §23). The per-issue time-entry collection: list newest first,
// log (create), single read, edit (patch), delete. Feed entries ride the
// existing history read (the server writes field "worklog" activities),
// and the sidebar total is a client-side sum over this list — the list
// is unpaged, so the sum is exact. No dedicated totals endpoint exists.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableDateTime, nullableUuid, uuid } from "./fields.js";
import { UserLite } from "./users.js";

/**
 * One logged time entry. Durations are whole minutes (never fractional
 * hours); logged_on is the calendar day the work happened, which may
 * differ from the day it was recorded.
 */
export const WorklogEntry = z.object({
  id: uuid(),
  issue: uuid(),
  project: uuid(),
  workspace: uuid(),
  actor: nullableUuid(),
  actor_detail: z.nullable(UserLite),
  minutes: z.number(),
  logged_on: z.iso.date(),
  note: z.string(),
  created_at: dateTime(),
  updated_at: dateTime(),
  created_by: z.nullable(z.string()),
  updated_by: z.nullable(z.string()),
  deleted_at: nullableDateTime(),
});
export type WorklogEntry = z.infer<typeof WorklogEntry>;

export const WorklogEntryList = z.array(WorklogEntry);
export type WorklogEntryList = z.infer<typeof WorklogEntryList>;

function worklogPath(slug: string, projectId: string, issueId: string): string {
  return `/api/workspaces/${slug}/projects/${projectId}/issues/${issueId}/worklogs/`;
}

export async function listWorklogEntries(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string
): Promise<WorklogEntryList> {
  const path = worklogPath(slug, projectId, issueId);
  const body = await client.getJson<unknown>(path);
  return client.parse(WorklogEntryList, body, `GET ${path}`);
}

export async function getWorklogEntry(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string,
  entryId: string
): Promise<WorklogEntry> {
  const path = `${worklogPath(slug, projectId, issueId)}${entryId}/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(WorklogEntry, body, `GET ${path}`);
}

export interface WorklogEntryCreate {
  /** Whole minutes of work; must be positive. */
  minutes: number;
  /** Calendar day the work happened (YYYY-MM-DD). */
  logged_on: string;
  /** Optional note describing the work. */
  note?: string;
}

export async function logWorklogEntry(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string,
  input: WorklogEntryCreate
): Promise<WorklogEntry> {
  const path = worklogPath(slug, projectId, issueId);
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(WorklogEntry, envelope.data, `POST ${path}`);
}

export interface WorklogEntryPatch {
  minutes?: number;
  logged_on?: string;
  note?: string;
}

export async function updateWorklogEntry(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string,
  entryId: string,
  input: WorklogEntryPatch
): Promise<WorklogEntry> {
  const path = `${worklogPath(slug, projectId, issueId)}${entryId}/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: input });
  return client.parse(WorklogEntry, envelope.data, `PATCH ${path}`);
}

export async function deleteWorklogEntry(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string,
  entryId: string
): Promise<void> {
  await client.delete(`${worklogPath(slug, projectId, issueId)}${entryId}/`);
}
