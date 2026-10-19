// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Profile page frame (parity/specs/profile.md §1 and §4, PROF-001/
// PROF-002/PROF-008). The layout reads the member's identity/per-project
// segregation once and hands it to the frame and every tab; it also warms
// the stats read the summary needs. Role comes from the session
// permission hook, never from an ad-hoc role read.

import { useQuery, useSuspenseQuery } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { getAppClient } from "../../../../../core/api/client.js";
import { useMe, usePermissions } from "../../../../../core/session/index.js";
import {
  profileSegregationQueryOptions,
  profileStatsQueryOptions,
} from "../../../../../features/profile/api/queries.js";
import { ProfileFrame } from "../../../../../features/profile/components/ProfileFrame.js";
import { projectsQueryOptions } from "../../../../../features/projects/index.js";

export const Route = createFileRoute("/$ws/_gated/profile/$userId")({
  loader: async ({ context, params }) => {
    await context.queryClient.ensureQueryData(
      profileSegregationQueryOptions(context.apiClient, params.ws, params.userId)
    );
    // The summary's cards are the first screen; warm them with the frame
    // so the tab does not flash placeholders on arrival.
    await context.queryClient
      .ensureQueryData(profileStatsQueryOptions(context.apiClient, params.ws, params.userId))
      .catch(() => undefined);
  },
  component: ProfileLayoutComponent,
});

function ProfileLayoutComponent(): React.ReactElement {
  const { ws, userId } = Route.useParams();
  const client = getAppClient();
  const permissions = usePermissions(ws);
  const me = useMe();
  const { data: segregation } = useSuspenseQuery(profileSegregationQueryOptions(client, ws, userId));
  const projects = useQuery(projectsQueryOptions(client, ws));
  const isOwn = me.data?.id === userId;
  return (
    <ProfileFrame
      workspaceSlug={ws}
      userId={userId}
      segregation={segregation}
      permissions={permissions}
      isOwn={isOwn}
      projects={(projects.data ?? []).map((project) => ({ id: project.id, name: project.name }))}
      apiBaseUrl={client.baseUrl}
    />
  );
}
