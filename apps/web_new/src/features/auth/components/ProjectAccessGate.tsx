// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project access gate (AUTH-024, from parity/specs/auth.md). Project
// routes render their content through this gate: while the details load
// nothing renders; members render children; everyone else gets the join,
// no-access, or not-found screen keyed off the details-fetch status plus
// admin standing. Joining refreshes the workspace subtree so the gate
// renders the project once the join lands.

import { isApiError } from "@pidash/api-client";
import { Button } from "@pidash/kit";
import * as React from "react";

import { ROLE_ADMIN } from "../../../core/session/permissions.js";
import { useJoinProject } from "../api.js";
import { decideProjectDenial } from "../denials.js";
import { authStepInputProps } from "../focus.js";
import { useProjectRow, useWorkspaceMembership } from "../queries.js";

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
    return (
      <DenialShell
        heading="You haven't joined this project yet"
        body="Join it to see its work."
        action={
          <Button
            variant="primary"
            loading={join.isPending}
            disabled={join.isPending}
            {...authStepInputProps()}
            onClick={() => join.mutate({ slug, projectId })}
          >
            {join.isPending ? "Joining…" : "Join project"}
          </Button>
        }
      />
    );
  }
  if (denial === "no-access") {
    return (
      <DenialShell
        heading="You can't open this project"
        body="This project is private. Ask a workspace admin for access to continue."
      />
    );
  }
  return <DenialShell heading="Project not found" body="It may have been deleted, or the address is wrong." />;
}

function DenialShell({
  heading,
  body,
  action,
}: {
  heading: string;
  body: string;
  action?: React.ReactNode | undefined;
}): React.ReactElement {
  return (
    <section
      aria-label={heading}
      className="flex h-full w-full flex-col items-center justify-center gap-(--space-2) p-(--space-12) text-center"
    >
      <h2 className="text-h3 text-(--text)">{heading}</h2>
      <p className="text-body max-w-md text-(--text-muted)">{body}</p>
      {action === undefined ? null : <div className="pt-(--space-2)">{action}</div>}
    </section>
  );
}
