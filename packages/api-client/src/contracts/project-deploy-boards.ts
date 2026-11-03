// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project deploy boards behind the publish dialog (parity/specs/
// project-settings-lifecycle.md §PSET-016): read the project's board
// settings, publish (create) the board with its layout and interaction
// flags, update it, and unpublish (delete). The board's `anchor` is the
// public link token; a project with no board answers an all-null shell.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableDateTime, nullableUuid, uuid } from "./fields.js";

/** Layout keys the publish dialog offers and the server stores. */
export const DEPLOY_BOARD_LAYOUTS = ["list", "kanban", "calendar", "gantt", "spreadsheet"] as const;

export const DeployBoardViewProps = z.record(z.string(), z.boolean());
export type DeployBoardViewProps = z.infer<typeof DeployBoardViewProps>;

/** Project projection embedded in a board row. */
export const DeployBoardProject = z.object({
  id: uuid(),
  identifier: z.string(),
  name: z.string(),
  cover_image: z.nullable(z.string()),
  cover_image_url: z.nullable(z.string()),
  logo_props: z.record(z.string(), z.unknown()),
  description: z.string(),
  is_default: z.boolean(),
});
export type DeployBoardProject = z.infer<typeof DeployBoardProject>;

export const DeployBoardWorkspace = z.object({
  name: z.string(),
  slug: z.string(),
  id: uuid(),
  logo_url: z.nullable(z.string()),
});
export type DeployBoardWorkspace = z.infer<typeof DeployBoardWorkspace>;

/**
 * One board row (serializer fields="__all__"). `entity_name` is "project"
 * for the publish dialog's board; `anchor` is the public token and null
 * before the first publish. `intake` points at a publish intake when the
 * dialog wires one (null otherwise).
 */
export const ProjectDeployBoard = z.object({
  id: uuid(),
  project_details: DeployBoardProject,
  workspace_detail: DeployBoardWorkspace,
  created_at: dateTime(),
  updated_at: dateTime(),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
  deleted_at: nullableDateTime(),
  workspace: uuid(),
  project: uuid(),
  entity_identifier: nullableUuid(),
  entity_name: z.nullable(z.string()),
  anchor: z.nullable(z.string()),
  is_comments_enabled: z.boolean(),
  is_reactions_enabled: z.boolean(),
  intake: nullableUuid(),
  is_votes_enabled: z.boolean(),
  view_props: DeployBoardViewProps,
  is_activity_enabled: z.boolean(),
  is_disabled: z.boolean(),
});
export type ProjectDeployBoard = z.infer<typeof ProjectDeployBoard>;

/**
 * The unpublish/empty shape. The view serializes `None`, so only the
 * model's own scalar fields survive: ids and the anchor are absent, JSON
 * fields degrade to null, booleans to false and the entity name to "". The
 * dialog treats an absent `anchor` as "not published".
 */
export const ProjectDeployBoardEmpty = z.object({
  deleted_at: z.null(),
  entity_identifier: z.null(),
  entity_name: z.string(),
  is_comments_enabled: z.boolean(),
  is_reactions_enabled: z.boolean(),
  is_votes_enabled: z.boolean(),
  view_props: z.null(),
  is_activity_enabled: z.boolean(),
  is_disabled: z.boolean(),
  created_by: z.null(),
  updated_by: z.null(),
  intake: z.null(),
});
export type ProjectDeployBoardEmpty = z.infer<typeof ProjectDeployBoardEmpty>;

export const ProjectDeployBoardStatus = z.union([ProjectDeployBoard, ProjectDeployBoardEmpty]);
export type ProjectDeployBoardStatus = z.infer<typeof ProjectDeployBoardStatus>;

/**
 * GET .../projects/<pid>/project-deploy-boards/ — the project's board, or
 * the all-null shell before it is published. Any project member.
 */
export async function getProjectDeployBoard(
  client: ApiClient,
  slug: string,
  projectId: string
): Promise<ProjectDeployBoardStatus> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/project-deploy-boards/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(ProjectDeployBoardStatus, body, `GET ${path}`);
}

export interface ProjectDeployBoardCreateInput {
  /** Layout selections by key; only the keys the dialog exposes matter. */
  views: Partial<Record<(typeof DEPLOY_BOARD_LAYOUTS)[number], boolean>>;
  is_comments_enabled?: boolean;
  is_reactions_enabled?: boolean;
  is_votes_enabled?: boolean;
  /** Publish-intake link when the project publishes into an intake. */
  intake?: string | null;
}

/**
 * POST .../project-deploy-boards/ creates or resets the project's board
 * (get-or-create on the project) and answers 200 with the row. The layout
 * map travels as `views`, not `view_props`; omitted flags default false.
 */
export async function createProjectDeployBoard(
  client: ApiClient,
  slug: string,
  projectId: string,
  input: ProjectDeployBoardCreateInput
): Promise<ProjectDeployBoard> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/project-deploy-boards/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(ProjectDeployBoard, envelope.data, `POST ${path}`);
}

export interface ProjectDeployBoardUpdateInput {
  view_props?: Partial<Record<(typeof DEPLOY_BOARD_LAYOUTS)[number], boolean>>;
  is_comments_enabled?: boolean;
  is_reactions_enabled?: boolean;
  is_votes_enabled?: boolean;
  is_activity_enabled?: boolean;
  is_disabled?: boolean;
  intake?: string | null;
}

/**
 * PATCH .../project-deploy-boards/<id>/ updates the flags and layouts and
 * answers 200 with the row. Unlike create, the layout map is stored as
 * `view_props` here (the model field the read returns).
 */
export async function updateProjectDeployBoard(
  client: ApiClient,
  slug: string,
  projectId: string,
  boardId: string,
  input: ProjectDeployBoardUpdateInput
): Promise<ProjectDeployBoard> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/project-deploy-boards/${boardId}/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: input });
  return client.parse(ProjectDeployBoard, envelope.data, `PATCH ${path}`);
}

/** DELETE .../project-deploy-boards/<id>/ unpublishes; resolves the status. */
export async function deleteProjectDeployBoard(
  client: ApiClient,
  slug: string,
  projectId: string,
  boardId: string
): Promise<number> {
  const response = await client.delete(
    `/api/workspaces/${slug}/projects/${projectId}/project-deploy-boards/${boardId}/`
  );
  return response.status;
}
