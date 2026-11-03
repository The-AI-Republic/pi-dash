// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Current-user read contracts (GET /api/users/me/, settings, and the
// user-lite projection). Display-name/avatar writes and the onboard
// marker live in user-writes.ts, out of the initial bundle
// (NEWFRONT-523, size gate).
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableDateTime, uuid } from "./fields.js";

export const Me = z.object({
  id: uuid(),
  email: z.email(),
  username: z.string(),
  first_name: z.string(),
  last_name: z.string(),
  display_name: z.string(),
  avatar: z.string(),
  avatar_url: z.nullable(z.string()),
  cover_image: z.nullable(z.string()),
  cover_image_url: z.nullable(z.string()),
  date_joined: dateTime(),
  user_timezone: z.string(),
  is_active: z.boolean(),
  is_bot: z.boolean(),
  is_email_verified: z.boolean(),
  is_password_autoset: z.boolean(),
  last_login_medium: z.string(),
  last_login_time: nullableDateTime(),
});
export type Me = z.infer<typeof Me>;

export async function getMe(client: ApiClient): Promise<Me> {
  const body = await client.getJson<unknown>("/api/users/me/");
  return client.parse(Me, body, "GET /api/users/me/");
}

/**
 * Small user projection embedded in members, issues and activity. The
 * backend answers null for users who never set an avatar, so the URL
 * leg stays nullable (matches the avatar_url model property).
 */
export const UserLite = z.object({
  id: uuid(),
  first_name: z.string(),
  last_name: z.string(),
  avatar: z.string(),
  avatar_url: z.nullable(z.string()),
  display_name: z.string(),
  is_bot: z.boolean(),
});
export type UserLite = z.infer<typeof UserLite>;

/** Last-workspace pointer returned with the user settings payload. */
export const MeSettingsWorkspace = z.object({
  last_workspace_id: z.nullable(z.union([uuid(), z.string()])),
  last_workspace_slug: z.nullable(z.string()),
  fallback_workspace_id: z.nullable(z.union([uuid(), z.string()])),
  fallback_workspace_slug: z.nullable(z.string()),
  invites: z.number(),
});
export type MeSettingsWorkspace = z.infer<typeof MeSettingsWorkspace>;

export const MeSettings = z.object({
  id: uuid(),
  email: z.email(),
  workspace: MeSettingsWorkspace,
});
export type MeSettings = z.infer<typeof MeSettings>;

export async function getMeSettings(client: ApiClient): Promise<MeSettings> {
  const body = await client.getJson<unknown>("/api/users/me/settings/");
  return client.parse(MeSettings, body, "GET /api/users/me/settings/");
}
