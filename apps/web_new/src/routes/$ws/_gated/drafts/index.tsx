// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace drafts route (parity/specs/issues.md §12, ISS-137–141). The
// route stays thin: it claims the document head, mounts the drafts page,
// and hosts the two dialogs the page opens from outside itself — the
// shared work-item dialog and the workspace create-project dialog. Both
// are heavy leaves loaded on first use so the drafts route's initial
// chunk stays small.

import { useQuery } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { getAppClient } from "../../../../core/api/client.js";
import { DraftsPage } from "../../../../features/issues/drafts/DraftsPage.js";
import { CreateProjectDialog, workspaceMembersQueryOptions } from "../../../../features/projects/index.js";
import { useDocumentHead } from "../../../../shared/shell/index.js";

export const DRAFTS_HEAD_TITLE = "Drafts";

const LazyCreateDialog = React.lazy(() =>
  import("../../../../features/issues/components/WorkItemCreateDialog.js").then((module) => ({
    default: module.WorkItemCreateDialog,
  }))
);

export const Route = createFileRoute("/$ws/_gated/drafts/")({
  component: DraftsRouteComponent,
});

function DraftsRouteComponent(): React.ReactElement {
  const { ws } = Route.useParams();
  useDocumentHead({ title: DRAFTS_HEAD_TITLE, robots: "noindex, nofollow" });
  return <DraftsRouteView key={ws} workspaceSlug={ws} />;
}

export interface DraftsRouteViewProps {
  workspaceSlug: string;
}

/** The page plus the dialogs it cannot mount itself. */
export function DraftsRouteView({ workspaceSlug }: DraftsRouteViewProps): React.ReactElement {
  const members = useQuery(workspaceMembersQueryOptions(getAppClient(), workspaceSlug));
  const [createProjectOpen, setCreateProjectOpen] = React.useState(false);
  return (
    <>
      <DraftsPage
        workspaceSlug={workspaceSlug}
        CreateDialog={LazyCreateDialog}
        onCreateProject={() => setCreateProjectOpen(true)}
      />
      <CreateProjectDialog
        workspaceSlug={workspaceSlug}
        members={members.data}
        open={createProjectOpen}
        onOpenChange={setCreateProjectOpen}
      />
    </>
  );
}
