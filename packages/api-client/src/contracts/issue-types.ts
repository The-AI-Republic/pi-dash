// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project work-item types: list and create. Types are workspace rows
// linked onto projects; the cloud type picker, type filter and detail
// badge all read through the project list (NEWFRONT-984). Reads are
// open to every active project member; creation is admin-only, like
// labels. The routes ride the OSS stack API ungated — the cloud gate
// lives in the web_new edition module, not here.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { uuid } from "./fields.js";

/** One work-item type row as the project list answers it. */
export const IssueTypeRow = z.object({
  id: uuid(),
  name: z.string(),
  description: z.string(),
  logo_props: z.record(z.string(), z.unknown()),
  is_epic: z.boolean(),
  is_default: z.boolean(),
  is_active: z.boolean(),
  level: z.number(),
});
export type IssueTypeRow = z.infer<typeof IssueTypeRow>;

/** The project type list: a bare array, like labels. */
export const IssueTypeList = z.array(IssueTypeRow);
export type IssueTypeList = z.infer<typeof IssueTypeList>;

export async function listIssueTypes(client: ApiClient, slug: string, projectId: string): Promise<IssueTypeList> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/issue-types/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(IssueTypeList, body, `GET ${path}`);
}

/** Mint a type in the workspace and link it onto the project. */
export interface IssueTypeCreateInput {
  name: string;
  description?: string;
  logo_props?: Record<string, unknown>;
  is_epic?: boolean;
  level?: number;
}

export async function createIssueType(
  client: ApiClient,
  slug: string,
  projectId: string,
  input: IssueTypeCreateInput
): Promise<IssueTypeRow> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/issue-types/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(IssueTypeRow, envelope.data, `POST ${path}`);
}
