// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project settings General tab reads and writes (PSET-005–009). The tab
// PATCHes the same endpoint as the lifecycle writes but needs the policy
// columns back (ticking switch, run pool, per-stage cadence) that the
// shared projection leaves off, so it declares the wider row here. This
// module is reached only from the settings route, never from the shell,
// keeping it out of the initial bundle (size gate).
import * as z from "zod/mini";

import type { ApiClient } from "../client.js";
import { ApiError, isApiError } from "../errors.js";
import { SignedUserAsset, type ByteUpload, type ByteUploadResponse } from "./assets.js";
import type { ProjectUpdateInput } from "./project-management.js";
import { findProjectDetailsRow, ProjectDetails } from "./projects.js";

/**
 * The project row the General tab reads: every shared field plus the
 * agent-policy columns the tab owns. The server sends the whole model.
 */
export const ProjectSettingsDetail = z.extend(ProjectDetails, {
  agent_ticking_enabled: z.boolean(),
  agent_default_max_ticks: z.number(),
  agent_default_interval_seconds: z.number(),
  agent_review_default_interval_seconds: z.number(),
  agent_test_default_interval_seconds: z.number(),
});
export type ProjectSettingsDetail = z.infer<typeof ProjectSettingsDetail>;

/**
 * GET /api/workspaces/<slug>/projects/<id>/ — the General tab's read.
 * Archived projects 404 on retrieve but stay on the rich list, so a
 * 404 falls back to the list row and only unknown ids stay not-found
 * (PSET-010), mirroring getProject.
 */
export async function getProjectSettingsDetail(
  client: ApiClient,
  slug: string,
  projectId: string
): Promise<ProjectSettingsDetail> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/`;
  try {
    const body = await client.getJson<unknown>(path);
    return client.parse(ProjectSettingsDetail, body, `GET ${path}`);
  } catch (error) {
    if (!isApiError(error) || error.status !== 404) throw error;
    const match = await findProjectDetailsRow(client, slug, projectId);
    if (match === undefined) throw error;
    return client.parse(ProjectSettingsDetail, match, `GET ${path} (archived fallback)`);
  }
}

/**
 * The same PATCH as updateProject, read back with the policy columns the
 * General tab owns. The server answers with the whole model, so the two
 * parsers differ only in how much of the answer each caller needs.
 */
export async function updateProjectSettings(
  client: ApiClient,
  slug: string,
  projectId: string,
  input: ProjectUpdateInput
): Promise<ProjectSettingsDetail> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: input });
  return client.parse(ProjectSettingsDetail, envelope.data, `PATCH ${path}`);
}

async function defaultByteUpload(url: string, body: FormData): Promise<ByteUploadResponse> {
  const response = await globalThis.fetch(url, { method: "POST", body });
  return { ok: response.ok, status: response.status };
}

/**
 * Upload one project-owned image and resolve to its public URL (PSET-007).
 *
 * The grant and confirm legs ride the workspace asset route
 * (`POST /api/assets/v2/workspaces/<slug>/`) with the project id as
 * `entity_identifier`, which is the shape the server's workspace handler
 * accepts: it assigns `project_id` once from that identifier and its
 * confirm leg wires the asset onto the project's cover slot. The
 * project-scoped route is deliberately not used: its handler passes
 * `project_id` both explicitly and through the entity-id lookup, which
 * raises before the grant is answered. Nothing here rewires the signed-in
 * account's profile. Any failed leg rejects with ApiError.
 */
export async function uploadProjectAsset(
  client: ApiClient,
  slug: string,
  projectId: string,
  entityType: "PROJECT_COVER",
  file: File,
  uploadBytes: ByteUpload = defaultByteUpload
): Promise<string> {
  const grantPath = `/api/assets/v2/workspaces/${slug}/`;
  const envelope = await client.sendJson<unknown>("POST", grantPath, {
    json: {
      entity_identifier: projectId,
      entity_type: entityType,
      name: file.name,
      size: file.size,
      type: file.type,
    },
  });
  const grant = client.parse(SignedUserAsset, envelope.data, `POST ${grantPath}`);

  const payload = new FormData();
  for (const [key, value] of Object.entries(grant.upload_data.fields)) {
    payload.append(key, value);
  }
  payload.append("file", file);
  const uploaded = await uploadBytes(grant.upload_data.url, payload);
  if (!uploaded.ok) {
    throw new ApiError({
      status: uploaded.status,
      code: "http",
      message: "The image upload was rejected.",
    });
  }

  await client.patch(`/api/assets/v2/workspaces/${slug}/${encodeURIComponent(grant.asset_id)}/`);
  return grant.asset_url;
}
