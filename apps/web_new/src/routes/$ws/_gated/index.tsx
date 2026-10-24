// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace home dashboard (NEWFRONT-278 top half, NEWFRONT-280 widget
// stack, NEWFRONT-279 quicklinks plus widget management). Renders the
// dashboard composition inside the gated shell: the tour seam stays
// mounted above it, and the widget stack — dashboard chrome with the
// recents and quicklinks sections — fills the dashboard's `widgets`
// slot. The loader warms the assistant card reads so first paint settles
// with data; the widget-map read stays off the loader so the dashboard
// skeleton shows on first paint. The component composes the
// cross-feature projects read features cannot make themselves, and
// hosts the shared create-project dialog for the quickstart card.

import { useQuery } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { WorkspaceHomeTour } from "../../../features/auth/components/WorkspaceHomeTour.js";
import { HomeAllOffState } from "../../../features/home/components/HomeAllOffState.js";
import { HomeDashboard } from "../../../features/home/components/HomeDashboard.js";
import { HomeWidgetStack } from "../../../features/home/components/HomeWidgetStack.js";
import { ManageWidgetsDialog } from "../../../features/home/components/ManageWidgetsDialog.js";
import { QuicklinksWidget } from "../../../features/home/components/QuicklinksWidget.js";
import { CreateProjectDialog } from "../../../features/projects/components/CreateProjectDialog.js";
import { projectsQueryOptions } from "../../../features/projects/api/queries.js";
import { workspaceMembersQueryOptions } from "../../../features/projects/api/members.js";

export const Route = createFileRoute("/$ws/_gated/")({
  loader: async ({ context, params }) => {
    // NOTE (NEWFRONT-523, size gate): the assistant reads stay behind a
    // dynamic import so the home query modules never join the initial
    // bundle — this loader only runs on dashboard navigation, after first
    // paint. The loader still resolves before render, as before.
    const { homeAssistantConfigQueryOptions, homeThreadsQueryOptions } =
      await import("../../../features/home/index.js");
    // Tolerated reads: guests and failures resolve through the assistant
    // card gates, which render nothing until their data is present.
    await Promise.all([
      context.queryClient
        .ensureQueryData(homeThreadsQueryOptions(context.apiClient, params.ws, true))
        .catch(() => undefined),
      context.queryClient
        .ensureQueryData(homeAssistantConfigQueryOptions(context.apiClient, params.ws))
        .catch(() => undefined),
    ]);
    return {};
  },
  component: function WorkspaceHomeComponent() {
    const params = Route.useParams();
    const projects = useQuery(projectsQueryOptions(getAppClient(), params.ws));
    const memberships = useQuery(workspaceMembersQueryOptions(getAppClient(), params.ws));
    const [createOpen, setCreateOpen] = React.useState(false);
    return (
      <>
        <WorkspaceHomeTour />
        <HomeDashboard
          workspaceSlug={params.ws}
          projects={projects.data}
          onCreateProject={() => setCreateOpen(true)}
          widgets={
            <HomeWidgetStack
              workspaceSlug={params.ws}
              projects={projects.data}
              quicklinks={<QuicklinksWidget workspaceSlug={params.ws} />}
              manageWidgets={(controls) => <ManageWidgetsDialog workspaceSlug={params.ws} {...controls} />}
              emptyState={<HomeAllOffState />}
            />
          }
        />
        <CreateProjectDialog
          workspaceSlug={params.ws}
          members={memberships.data}
          open={createOpen}
          onOpenChange={setCreateOpen}
        />
      </>
    );
  },
});
