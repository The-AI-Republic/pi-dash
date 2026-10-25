// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Estimates tab leaf (PSET-004/PSET-049): the admin gate, then the shared
// estimates surface with its loader, empty state and enable switch.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

// The settings shell barrel reaches every project tab, including this
// one's estimates screen. Await both chunks in the route so the shell and
// the tab still commit together, away from the measured entry.
const loadEstimatesTab = () =>
  Promise.all([
    import("../../../../../../features/project-settings/components/ProjectSettingsScreen.js"),
    import("../../../../../../features/project-settings/estimates/EstimatesTab.js"),
  ]).then(([screen, tab]) => ({ ProjectSettingsScreen: screen.ProjectSettingsScreen, EstimatesTab: tab.EstimatesTab }));
const LazyEstimatesTab = React.lazy(async () => {
  const { ProjectSettingsScreen, EstimatesTab } = await loadEstimatesTab();
  return {
    default: function EstimatesSettingsRoute() {
      const { ws, projectId } = Route.useParams();
      return (
        <ProjectSettingsScreen workspaceSlug={ws} projectId={projectId} tabKey="estimates">
          <EstimatesTab />
        </ProjectSettingsScreen>
      );
    },
  };
});

export const Route = createFileRoute("/$ws/_gated/settings/projects/$projectId/estimates")({
  beforeLoad: async () => {
    await loadEstimatesTab();
  },
  component: SettingsRouteComponent,
});

function SettingsRouteComponent(): React.ReactElement {
  return (
    <React.Suspense fallback={null}>
      <LazyEstimatesTab />
    </React.Suspense>
  );
}
