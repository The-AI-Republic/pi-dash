// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One project view row with its actions (parity/specs/views.md §6,
// VIEW-012, VIEW-017, VIEW-020). The row identity and the trailing
// star/menu cluster compose here; the publish dialog's open state is
// shared, so the row's Live marker and the menu entry open the same
// dialog. The marker click exists on cloud builds only (the publish
// slot); OSS rows never wire it.

import type { SavedView } from "@pidash/api-client";
import * as React from "react";

import { edition } from "../../../core/edition/index.js";
import type { ViewOwner } from "../lib/projectViews.js";
import { ProjectViewRow } from "./ProjectViewRow.js";
import { ProjectViewRowActions } from "./ProjectViewRowActions.js";

export interface ProjectViewRowItemProps {
  workspaceSlug: string;
  projectId: string;
  view: SavedView;
  /** Null when the roster is missing or the owner left: no avatar then. */
  owner: ViewOwner | null;
  /** The signed-in user id, or null while unresolved. */
  viewerId: string | null;
  /** The caller's project role, or null while unresolved. */
  projectRole: number | null;
  /** Cycle/module entries hide while their project features are off. */
  hideCycle: boolean;
  hideModule: boolean;
}

export function ProjectViewRowItem({
  workspaceSlug,
  projectId,
  view,
  owner,
  viewerId,
  projectRole,
  hideCycle,
  hideModule,
}: ProjectViewRowItemProps): React.ReactElement {
  const [publishOpen, setPublishOpen] = React.useState(false);
  const publishable = edition.slots?.viewPublish !== undefined;
  return (
    <ProjectViewRow
      workspaceSlug={workspaceSlug}
      projectId={projectId}
      view={view}
      owner={owner}
      onLiveMarkerClick={publishable ? () => setPublishOpen(true) : undefined}
      trailingControl={
        <ProjectViewRowActions
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          view={view}
          viewerId={viewerId}
          projectRole={projectRole}
          hideCycle={hideCycle}
          hideModule={hideModule}
          publishOpen={publishOpen}
          onPublishOpenChange={setPublishOpen}
        />
      }
    />
  );
}
