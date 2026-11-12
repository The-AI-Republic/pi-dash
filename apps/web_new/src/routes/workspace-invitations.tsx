// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace-invitation route (AUTH-027). Thin: search schema, single
// invitation screen. Public and query-driven: the invitation id, the
// workspace slug, and the emailed token arrive as search params and work
// signed in or out, so there is no gate here.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { asString } from "../core/session/index.js";
import { SingleInvitation } from "../features/auth/components/SingleInvitation.js";

export interface WorkspaceInvitationSearch {
  invitation_id?: string | undefined;
  slug?: string | undefined;
  token?: string | undefined;
}

/** Search schema: invalid params fall back to absent instead of throwing. */
export function parseWorkspaceInvitationSearch(search: Record<string, unknown>): WorkspaceInvitationSearch {
  const invitationId = asString(search["invitation_id"]);
  const slug = asString(search["slug"]);
  const token = asString(search["token"]);
  return {
    ...(invitationId === undefined ? {} : { invitation_id: invitationId }),
    ...(slug === undefined ? {} : { slug }),
    ...(token === undefined ? {} : { token }),
  };
}

export const Route = createFileRoute("/workspace-invitations")({
  validateSearch: parseWorkspaceInvitationSearch,
  component: function WorkspaceInvitationComponent() {
    const search = Route.useSearch();
    return <WorkspaceInvitationScreen search={search} />;
  },
});

export function WorkspaceInvitationScreen({ search }: { search: WorkspaceInvitationSearch }): React.ReactElement {
  return <SingleInvitation invitationId={search.invitation_id} slug={search.slug} token={search.token} />;
}
