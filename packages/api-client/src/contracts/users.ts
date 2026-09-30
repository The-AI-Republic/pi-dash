// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Current-user contract (GET /api/users/me/).
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { nullableDateTime, uuid } from "./fields.js";

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
  date_joined: z.iso.datetime(),
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

/** Small user projection embedded in members, issues and activity. */
export const UserLite = z.object({
  id: uuid(),
  first_name: z.string(),
  last_name: z.string(),
  avatar: z.string(),
  avatar_url: z.string(),
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
