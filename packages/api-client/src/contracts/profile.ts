// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Profile and locale contracts: the user's profile (theme, language,
// sidebar docking and other display preferences), the timezone field on the
// user itself, and the display list of known timezones.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { nullableUuid, uuid } from "./fields.js";

/**
 * The user profile. The server serializes every model field; JSON
 * preference bags arrive as objects, unset text as null.
 */
export const Profile = z.object({
  id: uuid(),
  user: uuid(),
  theme: z.record(z.string(), z.unknown()),
  is_app_rail_docked: z.boolean(),
  is_tour_completed: z.boolean(),
  onboarding_step: z.record(z.string(), z.unknown()),
  use_case: z.nullable(z.string()),
  role: z.nullable(z.string()),
  is_onboarded: z.boolean(),
  last_workspace_id: nullableUuid(),
  billing_address_country: z.string(),
  billing_address: z.nullable(z.record(z.string(), z.unknown())),
  has_billing_address: z.boolean(),
  company_name: z.string(),
  notification_view_mode: z.string(),
  is_smooth_cursor_enabled: z.boolean(),
  is_mobile_onboarded: z.boolean(),
  mobile_onboarding_step: z.record(z.string(), z.unknown()),
  mobile_timezone_auto_set: z.boolean(),
  language: z.string(),
  start_of_the_week: z.number(),
  goals: z.record(z.string(), z.unknown()),
  background_color: z.string(),
  is_navigation_tour_completed: z.boolean(),
  has_marketing_email_consent: z.boolean(),
  is_subscribed_to_changelog: z.boolean(),
  product_tour: z.record(z.string(), z.unknown()),
  settings: z.record(z.string(), z.unknown()),
  created_at: z.iso.datetime(),
  updated_at: z.iso.datetime(),
});
export type Profile = z.infer<typeof Profile>;

export async function getProfile(client: ApiClient): Promise<Profile> {
  const body = await client.getJson<unknown>("/api/users/me/profile/");
  return client.parse(Profile, body, "GET /api/users/me/profile/");
}

export interface ProfilePatch {
  theme?: Record<string, unknown>;
  language?: string;
  is_app_rail_docked?: boolean;
  is_tour_completed?: boolean;
  is_navigation_tour_completed?: boolean;
  start_of_the_week?: number;
  notification_view_mode?: string;
  is_smooth_cursor_enabled?: boolean;
}

export async function updateProfile(client: ApiClient, input: ProfilePatch): Promise<Profile> {
  const envelope = await client.sendJson<unknown>("PATCH", "/api/users/me/profile/", { json: input });
  return client.parse(Profile, envelope.data, "PATCH /api/users/me/profile/");
}

/** Thin projection of the user row after a timezone change. */
export const MeTimezone = z.object({
  id: uuid(),
  email: z.email(),
  user_timezone: z.string(),
});
export type MeTimezone = z.infer<typeof MeTimezone>;

export async function updateMyTimezone(client: ApiClient, timezone: string): Promise<MeTimezone> {
  const envelope = await client.sendJson<unknown>("PATCH", "/api/users/me/", {
    json: { user_timezone: timezone },
  });
  return client.parse(MeTimezone, envelope.data, "PATCH /api/users/me/");
}

export const TimezoneEntry = z.object({
  utc_offset: z.string(),
  gmt_offset: z.string(),
  value: z.string(),
  label: z.string(),
});
export type TimezoneEntry = z.infer<typeof TimezoneEntry>;

export const TimezoneList = z.object({
  timezones: z.array(TimezoneEntry),
});
export type TimezoneList = z.infer<typeof TimezoneList>;

export async function listTimezones(client: ApiClient): Promise<TimezoneList> {
  const body = await client.getJson<unknown>("/api/timezones/");
  return client.parse(TimezoneList, body, "GET /api/timezones/");
}
