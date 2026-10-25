// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// States tab leaf (PSET-004/PSET-028–035): the member gate plus the
// workflow-state screen (parity/specs/project-settings-states.md).

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

// The settings shell barrel reaches the estimates tab, and this tab's
// body reads the shared settings context through it. Awaiting both chunks
// in the route keeps that barrel out of the entry without a flash.
const loadStatesTab = () =>
  Promise.all([
    import("../../../../../../features/project-settings/components/ProjectSettingsScreen.js"),
    import("../../../../../../features/project-states/components/StatesTab.js"),
  ]).then(([screen, tab]) => ({ ProjectSettingsScreen: screen.ProjectSettingsScreen, StatesTab: tab.StatesTab }));
const LazyStatesTab = React.lazy(async () => {
  const { ProjectSettingsScreen, StatesTab } = await loadStatesTab();
  return {
    default: function StatesSettingsRoute() {
      const { ws, projectId } = Route.useParams();
      return (
        <ProjectSettingsScreen workspaceSlug={ws} projectId={projectId} tabKey="states">
          <StatesTab />
        </ProjectSettingsScreen>
      );
    },
  };
});

export const Route = createFileRoute("/$ws/_gated/settings/projects/$projectId/states")({
  beforeLoad: async () => {
    await loadStatesTab();
  },
  component: SettingsRouteComponent,
});

function SettingsRouteComponent(): React.ReactElement {
  return (
    <React.Suspense fallback={null}>
      <LazyStatesTab />
    </React.Suspense>
  );
}
