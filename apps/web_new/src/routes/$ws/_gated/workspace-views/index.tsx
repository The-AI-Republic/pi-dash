// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace views list route (parity/specs/views.md §9, VIEW-029-032/039).
// The gated layout already holds the session; this route only hands the
// workspace slug to the views-owned screen. Route modules are part of the
// eager route graph, so the screen and its read stay behind the router's
// component split.
//
// The address sits beside `all-issues`, whose own route mounts the global
// issue list: `/workspace-views` (no trailing segment) is the list, and a
// key or id after it is a view. The detail routes land with their slices.
//
// The row menus' edit and delete entries open the workspace dialogs
// (parity/specs/views.md §10, VIEW-035/036); the dialog reports closing
// only, so its handler clears the record it was opened with.

import { createFileRoute } from "@tanstack/react-router";
import type { SavedView } from "@pidash/api-client";
import * as React from "react";

// Deep imports: this route module is eager, so the views barrel would pin
// every dialog and screen it re-exports onto the initial chain
// (NEWFRONT-871, size gate).
import { WorkspaceViewsListScreen } from "../../../../features/views/components/WorkspaceViewsListScreen.js";
import { DeleteWorkspaceViewDialog } from "../../../../features/views/dialog/DeleteWorkspaceViewDialog.js";
import { WorkspaceViewDialog } from "../../../../features/views/dialog/WorkspaceViewDialog.js";

export const Route = createFileRoute("/$ws/_gated/workspace-views/")({
  component: WorkspaceViewsComponent,
});

function WorkspaceViewsComponent(): React.ReactElement {
  const { ws } = Route.useParams();
  const [editing, setEditing] = React.useState<SavedView | null>(null);
  const [deleting, setDeleting] = React.useState<SavedView | null>(null);
  return (
    <>
      <WorkspaceViewsListScreen workspaceSlug={ws} onEditView={setEditing} onDeleteView={setDeleting} />
      {editing !== null && (
        <WorkspaceViewDialog
          open={true}
          onOpenChange={() => setEditing(null)}
          workspaceSlug={ws}
          mode="edit"
          view={editing}
        />
      )}
      {deleting !== null && (
        <DeleteWorkspaceViewDialog
          open={true}
          onOpenChange={() => setDeleting(null)}
          workspaceSlug={ws}
          view={deleting}
        />
      )}
    </>
  );
}
