// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail gate (parity/specs/modules.md §8.1, §10.3, MOD-029, MOD-038).
// Wraps the module detail content and answers one question for the read
// behind it, through the single `moduleReadOutcome` split:
//
// - a live module renders the page;
// - an unresolvable id (404, or an archived record on the live address)
//   renders the gone-away view with its way back to the list;
// - a project-access refusal renders the shared project access wall, so a
//   visitor without access never sees module content and never reads a
//   refusal as "this module was deleted".
//
// The gate decides; it does not build a second wall or a second empty
// state.

import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { ROLE_ADMIN } from "../../../core/session/permissions.js";
import { useJoinProject, useWorkspaceMembership, ProjectAccessWall } from "../../auth/index.js";
import { moduleReadOutcome } from "../lib/moduleDetail.js";
import { modulesListHref } from "../lib/moduleList.js";
import { moduleDetailQueryOptions } from "./sidebar/moduleSidebarQueries.js";
import { ModuleGoneAway } from "./ModuleGoneAway.js";

export interface ModuleDetailGateProps {
  workspaceSlug: string;
  projectId: string;
  moduleId: string;
  children: React.ReactNode;
}

export function ModuleDetailGate({
  workspaceSlug,
  projectId,
  moduleId,
  children,
}: ModuleDetailGateProps): React.ReactElement {
  const navigate = useNavigate();
  const detail = useQuery(moduleDetailQueryOptions(getAppClient(), workspaceSlug, projectId, moduleId));
  const membership = useWorkspaceMembership(workspaceSlug);
  const join = useJoinProject();
  const isAdmin = (membership.data?.role ?? 0) >= ROLE_ADMIN;
  const outcome = moduleReadOutcome({
    error: detail.error,
    archivedAt: detail.data?.archived_at,
    isAdmin,
  });

  if (outcome.kind === "wall") {
    const canJoin = outcome.wall === "join";
    const joining = canJoin && join.isPending;
    return (
      <ProjectAccessWall
        kind={outcome.wall}
        joining={joining}
        onJoin={canJoin ? () => join.mutate({ slug: workspaceSlug, projectId }) : undefined}
      />
    );
  }
  if (outcome.kind === "missing") {
    return (
      <ModuleGoneAway
        onBack={() => {
          void navigate({ to: modulesListHref(workspaceSlug, projectId) });
        }}
      />
    );
  }
  // Every other failure keeps the route's own pending/error surface; the
  // gate never turns one into a missing-module notice.
  return <>{children}</>;
}
