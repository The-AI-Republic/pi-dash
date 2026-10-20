// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Profile page frame (parity/specs/profile.md §1 and §4, PROF-001/
// PROF-002/PROF-008). The layout reads the member's identity/per-project
// segregation and the workspace project names once and hands them to the
// frame and every tab. The summary's reads are warmed without blocking:
// the route component does not mount until its loader settles, so a
// pending stats request must never gate the frame or its placeholders.
// Role comes from the session permission hook, never from an ad-hoc read.

import { useSuspenseQuery } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { getAppClient } from "../../../../../core/api/client.js";
import { useMe, usePermissions } from "../../../../../core/session/index.js";
import { profileSegregationQueryOptions } from "../../../../../features/profile/api/queries.js";
import { ProfileFrame } from "../../../../../features/profile/components/ProfileFrame.js";
import { projectsQueryOptions } from "../../../../../features/projects/index.js";

export const Route = createFileRoute("/$ws/_gated/profile/$userId")({
  loader: async ({ context, params }) => {
    // The profile reads ride a lazy module: the loader itself stays in
    // the eager route file, and the first-route bundle budget counts
    // everything that file imports (NEWFRONT-573).
    const { profileSegregationQueryOptions, profileStatsQueryOptions } =
      await import("../../../../../features/profile/api/queries.js");
    await Promise.all([
      context.queryClient.ensureQueryData(profileSegregationQueryOptions(context.apiClient, params.ws, params.userId)),
      // Sidebar project rows carry names, so the frame waits for the
      // collection instead of painting ids first.
      context.queryClient.ensureQueryData(projectsQueryOptions(context.apiClient, params.ws)).catch(() => undefined),
    ]);
    // Warm the summary so its cards usually arrive with the frame; a slow
    // or failed read leaves the overview on its placeholders.
    void context.queryClient.prefetchQuery(profileStatsQueryOptions(context.apiClient, params.ws, params.userId));
  },
  component: ProfileLayoutComponent,
});

function ProfileLayoutComponent(): React.ReactElement {
  const { ws, userId } = Route.useParams();
  const client = getAppClient();
  const permissions = usePermissions(ws);
  const me = useMe();
  const { data: segregation } = useSuspenseQuery(profileSegregationQueryOptions(client, ws, userId));
  const { data: projects } = useSuspenseQuery(projectsQueryOptions(client, ws));
  const isOwn = me.data?.id === userId;
  return (
    <ProfileFrame
      workspaceSlug={ws}
      userId={userId}
      segregation={segregation}
      permissions={permissions}
      isOwn={isOwn}
      projects={projects.map((project) => ({ id: project.id, name: project.name }))}
      apiBaseUrl={client.baseUrl}
    />
  );
}
