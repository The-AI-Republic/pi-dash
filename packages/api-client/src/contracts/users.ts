// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
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

export interface MePatch {
  first_name?: string;
  last_name?: string;
  avatar?: string;
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
 * PATCH /api/users/me/ writes the display name and avatar. The avatar is
 * the `avatar` text field (a URL sticks and reads back through
 * `avatar_url`); an `avatar_url` key is silently ignored by the server.
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
