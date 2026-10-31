// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Auto project management (upkeep): the instance's enabled jobs with the
// caller's own on/off switches, one master pause and per-job toggles.
// The payload is a public whitelist — prompts and internal naming never
// travel, so the schema accepts exactly what the settings surface shows.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";

/** One upkeep job card. */
export const UpkeepJob = z.object({
  slug: z.string(),
  name: z.string(),
  description: z.string(),
  interval_label: z.string(),
  enabled: z.boolean(),
});
export type UpkeepJob = z.infer<typeof UpkeepJob>;

/** The whole upkeep settings payload; absence of a preference means on. */
export const UpkeepSettings = z.object({
  enabled: z.boolean(),
  jobs: z.array(UpkeepJob),
});
export type UpkeepSettings = z.infer<typeof UpkeepSettings>;

/** GET /api/users/me/auto-pm/ */
export async function getUpkeepSettings(client: ApiClient): Promise<UpkeepSettings> {
  const body = await client.getJson<unknown>("/api/users/me/auto-pm/");
  return client.parse(UpkeepSettings, body, "GET /api/users/me/auto-pm/");
}

/**
 * PATCH /api/users/me/auto-pm/ with only the master flag. The server
 * rejects any other key in the body, and answers the updated payload.
 */
export async function setUpkeepEnabled(client: ApiClient, enabled: boolean): Promise<UpkeepSettings> {
  const path = "/api/users/me/auto-pm/";
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: { enabled } });
  return client.parse(UpkeepSettings, envelope.data, `PATCH ${path}`);
}

/**
 * PATCH /api/users/me/auto-pm/jobs/<slug>/ with only that job's flag.
 * Unknown slugs answer 404; the response is the refreshed payload.
 */
export async function setUpkeepJobEnabled(client: ApiClient, slug: string, enabled: boolean): Promise<UpkeepSettings> {
  const path = `/api/users/me/auto-pm/jobs/${encodeURIComponent(slug)}/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: { enabled } });
  return client.parse(UpkeepSettings, envelope.data, `PATCH ${path}`);
}
