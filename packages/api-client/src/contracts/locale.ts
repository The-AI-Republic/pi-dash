// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Locale contracts: the timezone field on the user and the display list
// of known timezones. The profile read and write stay in profile.ts;
// everything here runs from the lazy preference surfaces, so it stays
// out of the initial bundle (NEWFRONT-523, size gate).
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { uuid } from "./fields.js";

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
