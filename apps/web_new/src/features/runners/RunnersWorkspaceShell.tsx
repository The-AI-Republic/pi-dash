// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace-scope runners shell (NEWFRONT-540, RUN-001/002/004). Resolves
// the workspace, the caller's role and the runner list, then hands the
// shell its access decision and contacts. The tab strip's active state
// follows the router's pathname.

import { useLocation } from "@tanstack/react-router";
import * as React from "react";

import { usePermissions } from "../../core/session/index.js";
import { decideWorkspaceRunnersAccess } from "./access.js";
import { buildRunnersAreaValue, RunnersAreaProvider } from "./area.js";
import { useRunnersRail } from "./api/queries.js";
import { RunnersAreaShell } from "./components/RunnersAreaShell.js";
import { useLocalContact } from "./chat/localContact.js";

export interface RunnersWorkspaceShellProps {
  workspaceSlug: string;
  /** Workspace id, read from the session and required by the list reads. */
  workspaceId: string;
  children: React.ReactNode;
}

export function RunnersWorkspaceShell({
  workspaceSlug,
  workspaceId,
  children,
}: RunnersWorkspaceShellProps): React.ReactElement | null {
  const permissions = usePermissions(workspaceSlug);
  const access = decideWorkspaceRunnersAccess({ role: permissions.role, isLoading: permissions.isLoading });
  const { pathname } = useLocation();
  const localContacts = useLocalContact();
  // Access first: a denied viewer never starts the rail read.
  const rail = useRunnersRail(workspaceSlug, workspaceId, null, access === "allowed");
  const area = buildRunnersAreaValue({ workspaceSlug }, workspaceId, access);
  return (
    <RunnersAreaProvider value={area}>
      <RunnersAreaShell
        scope={area.scope}
        access={access}
        runners={rail.data ?? []}
        pathname={pathname}
        localContacts={localContacts}
      >
        {children}
      </RunnersAreaShell>
    </RunnersAreaProvider>
  );
}
