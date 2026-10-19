// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Profile summary tab (parity/specs/profile.md §1, PROF-003/004/005).
// Reads the stats and the member's recent activity; the frame already
// resolved the identity. Both reads tolerate failure so the summary still
// renders its cards and empty states.

import { useQuery } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { getAppClient } from "../../../../../core/api/client.js";
import { useMe } from "../../../../../core/session/index.js";
import {
  PROFILE_OVERVIEW_ACTIVITY_SIZE,
  profileActivityQueryOptions,
  profileStatsQueryOptions,
} from "../../../../../features/profile/api/queries.js";
import { ProfileOverview } from "../../../../../features/profile/components/ProfileOverview.js";

export const Route = createFileRoute("/$ws/_gated/profile/$userId/")({
  loader: async ({ context, params }) => {
    // Both reads render behind placeholders, so neither may hold the
    // route: a slow request leaves the frame, cards and skeletons up.
    void context.queryClient.prefetchQuery(profileStatsQueryOptions(context.apiClient, params.ws, params.userId));
    void context.queryClient.prefetchQuery(
      profileActivityQueryOptions(context.apiClient, params.ws, params.userId, PROFILE_OVERVIEW_ACTIVITY_SIZE)
    );
  },
  component: ProfileSummaryComponent,
});

function ProfileSummaryComponent(): React.ReactElement {
  const { ws, userId } = Route.useParams();
  const client = getAppClient();
  const me = useMe();
  const stats = useQuery({
    ...profileStatsQueryOptions(client, ws, userId),
    throwOnError: false,
  });
  const activity = useQuery({
    ...profileActivityQueryOptions(client, ws, userId, PROFILE_OVERVIEW_ACTIVITY_SIZE),
    throwOnError: false,
  });
  return (
    <ProfileOverview
      workspaceSlug={ws}
      userId={userId}
      stats={stats.data}
      activity={activity.data?.results}
      currentUserId={me.data?.id ?? null}
    />
  );
}
