// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace home dashboard (NEWFRONT-278). Renders the dashboard top half
// (greeting, assistant card, quickstart) inside the gated shell: the tour
// seam stays mounted above it, and the widget slot below fills in with
// the sibling home slices (NEWFRONT-279, NEWFRONT-280). The loader warms
// the assistant card reads so first paint settles with data; the component
// composes the cross-feature projects read features cannot make themselves.

import { useQuery } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";

import { getAppClient } from "../../../core/api/client.js";
import { WorkspaceHomeTour } from "../../../features/auth/index.js";
import {
  HomeDashboard,
  homeAssistantConfigQueryOptions,
  homeThreadsQueryOptions,
} from "../../../features/home/index.js";
import { projectsQueryOptions } from "../../../features/projects/index.js";

export const Route = createFileRoute("/$ws/_gated/")({
  loader: async ({ context, params }) => {
    // Tolerated reads: guests and failures resolve through the assistant
    // card gates, which render nothing until their data is present.
    await Promise.all([
      context.queryClient
        .ensureQueryData(homeThreadsQueryOptions(context.apiClient, params.ws, true))
        .catch(() => undefined),
      context.queryClient
        .ensureQueryData(homeAssistantConfigQueryOptions(context.apiClient, params.ws, true))
        .catch(() => undefined),
    ]);
    return {};
  },
  component: function WorkspaceHomeComponent() {
    const params = Route.useParams();
    const projects = useQuery(projectsQueryOptions(getAppClient(), params.ws));
    return (
      <>
        <WorkspaceHomeTour />
        <HomeDashboard workspaceSlug={params.ws} projects={projects.data} />
      </>
    );
  },
});
