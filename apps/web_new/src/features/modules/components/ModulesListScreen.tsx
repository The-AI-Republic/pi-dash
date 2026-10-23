// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project modules list (parity/specs/modules.md §2.1, §2.2 and §3.1,
// MOD-001/002/008/011). Reads the project facts for the name, the feature
// switch and the zone, the modules collection for the body and the role
// map for the gate's shortcut; sets the tab title; and composes exactly one
// body state — gate, skeleton or content. The facts read is the modules
// feature's own tolerant projection: a project record without
// `module_view` parses and reads as off, so it gates instead of leaving
// the skeleton up (MOD-002). No list read runs while the gate shows, and
// the gate never flashes a skeleton first. The layout is the viewer's
// per-project choice, restored from the modules feature's own browser
// record.

import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { useDocumentHead, useNarrowViewport } from "../../../shared/shell/index.js";
import { useProjectRoles } from "../../auth/index.js";
import { isProjectAdmin } from "../../projects/index.js";
import { moduleProjectFactsQueryOptions, moduleViewEnabled } from "../api/projectFacts.js";
import { modulesListQueryOptions } from "../api/queries.js";
import { MODULES_CRUMB, modulesFeatureHref, modulesTitle, resolveModulesListBody } from "../lib/moduleList.js";
import { resolveCompactLayout } from "../lib/moduleLayouts.js";
import { useModuleLayout } from "../lib/listLayoutStore.js";
import { ModulesFeatureGate } from "./ModulesFeatureGate.js";
import { ModulesListBody } from "./ModulesListBody.js";
import { ModulesListHeader } from "./ModulesListHeader.js";
import { ModulesListSkeleton } from "./ModulesListSkeleton.js";

export interface ModulesListScreenProps {
  workspaceSlug: string;
  projectId: string;
  /**
   * The list body. Absent, the screen renders its own rows, cards or
   * timeline rows; slices B and F wrap this set with narrowing and the
   * empty containers.
   */
  children?: React.ReactNode;
}

export function ModulesListScreen({ workspaceSlug, projectId, children }: ModulesListScreenProps): React.ReactElement {
  const navigate = useNavigate();
  const narrow = useNarrowViewport();
  const project = useQuery(moduleProjectFactsQueryOptions(getAppClient(), workspaceSlug, projectId));
  // Only a literal `true` opens the list; an absent flag is off (§2.2).
  const gateOff = project.data !== undefined && !moduleViewEnabled(project.data.module_view);
  const modules = useQuery({
    ...modulesListQueryOptions(getAppClient(), workspaceSlug, projectId),
    enabled: project.isSuccess && !gateOff,
  });
  const roles = useProjectRoles(workspaceSlug);
  const { layout, setLayout } = useModuleLayout(projectId);

  useDocumentHead({
    title: project.data === undefined ? MODULES_CRUMB : modulesTitle(project.data.name),
    robots: "noindex, nofollow",
  });

  const state = resolveModulesListBody({
    projectPending: project.isPending,
    gateOff,
    modulesPending: modules.isPending,
  });
  // Small screens never render the timeline from the list; the stored
  // choice survives for the next wide visit.
  const shownLayout = narrow ? resolveCompactLayout(layout) : layout;

  return (
    <section
      aria-label={MODULES_CRUMB}
      data-testid="modules-list-screen"
      className="flex min-h-0 flex-1 flex-col gap-(--space-4) p-(--space-4)"
    >
      {state === "gate" || project.data === undefined ? null : (
        <ModulesListHeader projectName={project.data.name} layout={layout} onLayoutChange={setLayout} />
      )}
      {state === "gate" ? (
        <ModulesFeatureGate
          isAdmin={isProjectAdmin(roles.data?.[projectId] ?? null)}
          onManageFeatures={() => {
            void navigate({ to: modulesFeatureHref(workspaceSlug, projectId) });
          }}
        />
      ) : state === "skeleton" ? (
        <ModulesListSkeleton layout={shownLayout} />
      ) : (
        (children ?? (
          <ModulesListBody
            workspaceSlug={workspaceSlug}
            projectId={projectId}
            layout={shownLayout}
            modules={modules.data ?? []}
          />
        ))
      )}
    </section>
  );
}
