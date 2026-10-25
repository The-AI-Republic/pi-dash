// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace route. Thin: membership gate only, then the outlet. Known
// workspace pages additionally pass the sign-in gate in the `_gated`
// layout; the workspace catch-all (`/$ws/$`) is not a child of that
// layout, so unknown addresses render the illustrated not-found page with
// or without a session. Signed-in visitors on an unknown slug see the
// workspace-not-found screen, and members missing their membership info
// see the not-a-member screen (AUTH-024 Case A/B).

import { createFileRoute, notFound, Outlet } from "@tanstack/react-router";
import * as React from "react";

import {
  decideWorkspaceRouteGate,
  meQueryOptions,
  selectWorkspace,
  useMe,
  useWorkspaces,
  workspacesQueryOptions,
} from "../../core/session/index.js";
import { decideWorkspaceDenial } from "../../features/auth/denials.js";
import { membershipQueryOptions, projectRolesQueryOptions } from "../../features/auth/queries.js";
import { WorkspaceNotFoundScreen } from "../../features/auth/components/WorkspaceNotFoundScreen.js";
import { WorkspaceNotMemberScreen } from "../../features/auth/components/WorkspaceNotMemberScreen.js";

export const Route = createFileRoute("/$ws")({
  beforeLoad: async ({ context, params }) => {
    const me = await context.queryClient.ensureQueryData(meQueryOptions(context.apiClient)).catch(() => null);
    const workspaces = me
      ? await context.queryClient.ensureQueryData(workspacesQueryOptions(context.apiClient)).catch(() => undefined)
      : undefined;
    if (decideWorkspaceRouteGate({ signedIn: me !== null, workspaces, slugOrId: params.ws }) === "not-found") {
      throw notFound();
    }
    if (me !== null) {
      // The gate passed, so the slug is known: read the membership (the
      // Case B split; a failed read denies like a missing one) and warm
      // the roles feed (a failed warm is tolerated, never a denial).
      const membership = await context.queryClient
        .ensureQueryData(membershipQueryOptions(context.apiClient, params.ws))
        .catch(() => null);
      await context.queryClient
        .ensureQueryData(projectRolesQueryOptions(context.apiClient, params.ws))
        .catch(() => undefined);
      if (decideWorkspaceDenial({ known: true, membershipPresent: membership !== null }) !== null) {
        throw notFound();
      }
    }
    return {};
  },
  notFoundComponent: WorkspaceDenialNotice,
  component: WorkspaceRouteComponent,
});

function WorkspaceDenialNotice(): React.ReactElement | null {
  const params = Route.useParams();
  const me = useMe();
  const workspaces = useWorkspaces();
  if (me.isPending || workspaces.isPending) return null;
  const list = workspaces.data ?? [];
  if (selectWorkspace(list, params.ws) === undefined) {
    return <WorkspaceNotFoundScreen email={me.data?.email ?? ""} workspaceCount={list.length} />;
  }
  return <WorkspaceNotMemberScreen />;
}

function WorkspaceRouteComponent(): React.ReactElement {
  return <Outlet />;
}
