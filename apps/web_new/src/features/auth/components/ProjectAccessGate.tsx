// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project access gate (AUTH-024, from parity/specs/auth.md). Project
// routes render their content through this gate: while the details load
// nothing renders; members render children; everyone else gets the join,
// no-access, or not-found screen keyed off the details-fetch status plus
// admin standing. Joining refreshes the workspace subtree so the gate
// renders the project once the join lands. The wall itself is
// `ProjectAccessWall`, shared with areas that meet a refusal below the
// project shell (the modules detail gate).

import { isApiError } from "@pidash/api-client";
import * as React from "react";

import { ROLE_ADMIN } from "../../../core/session/permissions.js";
import { useJoinProject } from "../api.js";
import { decideProjectDenial } from "../denials.js";
import { useProjectRow, useWorkspaceMembership } from "../queries.js";
import { ProjectAccessWall } from "./ProjectAccessWall.js";

export interface ProjectAccessGateProps {
  slug: string;
  projectId: string;
  children: React.ReactNode;
}

export function ProjectAccessGate({ slug, projectId, children }: ProjectAccessGateProps): React.ReactElement | null {
  const project = useProjectRow(slug, projectId);
  const membership = useWorkspaceMembership(slug);
  const join = useJoinProject();

  if (project.isPending || membership.isPending) return null;
  if (project.isSuccess) return <>{children}</>;

  const status = isApiError(project.error) ? project.error.status : undefined;
  const denial = decideProjectDenial({ status, isAdmin: (membership.data?.role ?? 0) >= ROLE_ADMIN });
  if (denial === "join") {
    return <ProjectAccessWall kind="join" joining={join.isPending} onJoin={() => join.mutate({ slug, projectId })} />;
  }
  return <ProjectAccessWall kind={denial} />;
}
