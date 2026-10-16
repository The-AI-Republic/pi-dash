// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Profile contracts: the user's profile (theme, language, sidebar
// docking and other display preferences). The timezone field and the
// timezone list live in locale.ts, out of the initial bundle
// (NEWFRONT-523, size gate).
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableUuid, uuid } from "./fields.js";

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
  created_at: dateTime(),
  updated_at: dateTime(),
});
export type Profile = z.infer<typeof Profile>;

export async function getProfile(client: ApiClient): Promise<Profile> {
  const body = await client.getJson<unknown>("/api/users/me/profile/");
  return client.parse(Profile, body, "GET /api/users/me/profile/");
}

/**
 * The catalog's namespaced app settings: whether agent runs this user
 * starts may call the tools of their installed apps. Unset reads as
 * off — enabling is a deliberate act, never inherited. The server
 * merges settings per namespace, so patching this namespace cannot
 * drop another surface's values.
 */
export const OpenhubAppSettings = z.object({
  apps_enabled: z.boolean(),
});
export type OpenhubAppSettings = z.infer<typeof OpenhubAppSettings>;

/**
 * Read the agent-tools flag out of a profile settings bag. Anything but
 * an explicit true in the catalog namespace reads as off.
 */
export function agentToolsEnabled(settings: Profile["settings"]): boolean {
  const openhub = settings["openhub"];
  if (typeof openhub !== "object" || openhub === null) return false;
  return (openhub as Record<string, unknown>)["apps_enabled"] === true;
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
  role?: string | null;
  use_case?: string | null;
  has_marketing_email_consent?: boolean;
  onboarding_step?: Record<string, unknown>;
  last_workspace_id?: string | null;
  settings?: { openhub?: { apps_enabled?: boolean } };
}

export async function updateProfile(client: ApiClient, input: ProfilePatch): Promise<Profile> {
  const envelope = await client.sendJson<unknown>("PATCH", "/api/users/me/profile/", { json: input });
  return client.parse(Profile, envelope.data, "PATCH /api/users/me/profile/");
}
