// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Current-user writes: the display-name/avatar update and the onboard
// marker. The me, settings and user-lite reads stay in users.ts;
// everything here runs from lazy surfaces (onboarding, the profile
// setup), so it stays out of the initial bundle (NEWFRONT-523, size
// gate).
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { uuid } from "./fields.js";

export interface MePatch {
  first_name?: string;
  last_name?: string;
  display_name?: string;
  avatar?: string;
  user_timezone?: string;
}

/**
 * The user row after an update. PATCH answers the full user serializer,
 * which lacks the computed avatar/cover URLs the GET shape carries, so
 * this is its own projection, not Me.
 */
export const MeUpdate = z.object({
  id: uuid(),
  email: z.email(),
  username: z.string(),
  first_name: z.string(),
  last_name: z.string(),
  display_name: z.string(),
  avatar: z.string(),
  user_timezone: z.string(),
});
export type MeUpdate = z.infer<typeof MeUpdate>;

/**
 * PATCH /api/users/me/ writes the names, avatar and timezone. The avatar
 * is the `avatar` text field (a URL sticks and reads back through
 * `avatar_url`); an `avatar_url` key is silently ignored by the server.
 * The timezone write lives here too, so the account form and the locale
 * selector share one call path; `updateMyTimezone` in locale.ts keeps its
 * narrower projection for callers that only read the zone back.
 */
export async function updateMe(client: ApiClient, input: MePatch): Promise<MeUpdate> {
  const envelope = await client.sendJson<unknown>("PATCH", "/api/users/me/", { json: input });
  return client.parse(MeUpdate, envelope.data, "PATCH /api/users/me/");
}

export const OnboardResult = z.object({
  message: z.string(),
});
export type OnboardResult = z.infer<typeof OnboardResult>;

/** PATCH /api/users/me/onboard/ stamps the profile onboard marker. */
export async function markOnboarded(client: ApiClient, input: { is_onboarded: boolean }): Promise<OnboardResult> {
  const path = "/api/users/me/onboard/";
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: input });
  return client.parse(OnboardResult, envelope.data, `PATCH ${path}`);
}
