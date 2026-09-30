// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Workspace home (first vertical slice). Forwards into the first project
// until the dashboard lands with NEWFRONT-70.

import { createFileRoute, redirect } from "@tanstack/react-router";

import { projectsQueryOptions } from "../../features/projects/index.js";

export const Route = createFileRoute("/$ws/")({
  loader: async ({ context, params }) => {
    const projects = await context.queryClient
      .ensureQueryData(projectsQueryOptions(context.apiClient, params.ws))
      .catch(() => []);
    const first = projects[0];
    if (first) {
      throw redirect({
        to: "/$ws/projects/$projectId/issues",
        params: { ws: params.ws, projectId: first.id },
      });
    }
    return {};
  },
  component: function WorkspaceHomeComponent() {
    return (
      <section aria-label="Workspace home" className="flex flex-col items-center gap-(--space-2) p-(--space-12)">
        <h2 className="text-h3 text-(--text)">No projects yet</h2>
        <p className="text-body text-(--text-muted)">Projects in this workspace will appear in the sidebar.</p>
      </section>
    );
  },
});
