// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Auth queries. The card drives the steps; components read server data
// through these hooks, never raw client calls.

import { queryOptions, useQuery } from "@tanstack/react-query";
import { getWorkspaceInvitation, type ApiClient } from "@pidash/api-client";

import { getAppClient } from "../../core/api/client.js";

export const authKeys = {
  invitation: (slug: string, invitationId: string) => ["auth", "invitation", slug, invitationId] as const,
};

/** One emailed invitation for the invitation-aware card header (AUTH-005). */
export function invitationQueryOptions(client: ApiClient, slug: string, invitationId: string) {
  return queryOptions({
    queryKey: authKeys.invitation(slug, invitationId),
    queryFn: () => getWorkspaceInvitation(client, slug, invitationId),
    enabled: slug.length > 0 && invitationId.length > 0,
  });
}

/** The emailed invitation when the card URL carries one; disabled otherwise. */
export function useWorkspaceInvitation(slug?: string | undefined, invitationId?: string | undefined) {
  return useQuery(invitationQueryOptions(getAppClient(), slug ?? "", invitationId ?? ""));
}
