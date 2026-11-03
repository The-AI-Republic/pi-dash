// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The first-run tour flag (AUTH-042), kept apart from the home contracts:
// the account gate reads it through the auth feature, which loads with the
// shell, and the home dashboard's dashboard/preferences/quick-link/recents
// shapes should stay out of that first paint (size gate, NEWFRONT-579).
import * as z from "zod/mini";
import type { ApiClient } from "../client.js";

export const TourCompletedResponse = z.object({
  message: z.string(),
});
export type TourCompletedResponse = z.infer<typeof TourCompletedResponse>;

/** PATCH /api/users/me/tour-completed/ flips the profile tour flag. */
export async function completeTour(
  client: ApiClient,
  input: { is_tour_completed: boolean }
): Promise<TourCompletedResponse> {
  const path = "/api/users/me/tour-completed/";
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: input });
  return client.parse(TourCompletedResponse, envelope.data, `PATCH ${path}`);
}
