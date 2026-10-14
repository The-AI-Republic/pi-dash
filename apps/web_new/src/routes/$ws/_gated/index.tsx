// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace home (first vertical slice). Interim project list until the
// dashboard lands with NEWFRONT-70: the home URL stays put (route guards
// and landings bounce here, AUTH-015/AUTH-016) instead of forwarding into
// the first project. The product-tour seam (AUTH-042, owned by the auth
// area) stays mounted here: later home UI keeps it, passing the
// create-project dialog opener once it exists.

import { createFileRoute, Link } from "@tanstack/react-router";

import { WorkspaceHomeTour } from "../../../features/auth/index.js";
import { projectsQueryOptions } from "../../../features/projects/index.js";

export const Route = createFileRoute("/$ws/_gated/")({
  loader: async ({ context, params }) => {
    const projects = await context.queryClient
      .ensureQueryData(projectsQueryOptions(context.apiClient, params.ws))
      .catch(() => []);
    return { projects };
  },
  component: function WorkspaceHomeComponent() {
    const { projects } = Route.useLoaderData();
    const params = Route.useParams();
    return (
      <>
        <WorkspaceHomeTour />
        {projects.length === 0 ? (
          <section aria-label="Workspace home" className="flex flex-col items-center gap-(--space-2) p-(--space-12)">
            <h2 className="text-h3 text-(--text)">No projects yet</h2>
            <p className="text-body text-(--text-muted)">Projects in this workspace will appear in the sidebar.</p>
          </section>
        ) : (
          <section aria-label="Workspace home" className="flex flex-col gap-(--space-2) p-(--space-12)">
            <h2 className="text-h3 text-(--text)">Projects</h2>
            <ul className="flex flex-col gap-(--space-1)">
              {projects.map((project) => (
                <li key={project.id}>
                  <Link
                    to="/$ws/projects/$projectId/issues"
                    params={{ ws: params.ws, projectId: project.id }}
                    className="text-emphasis text-(--text)"
                  >
                    {project.name}
                  </Link>
                </li>
              ))}
            </ul>
          </section>
        )}
      </>
    );
  },
});
