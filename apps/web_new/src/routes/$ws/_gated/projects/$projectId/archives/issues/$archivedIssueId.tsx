// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archived-issue detail route (parity/specs/archives.md §1 and §3.5).
// The address exists in the shell so the detail slice can mount its
// content; the frame carries the breadcrumb trail without the tab
// strip. Banner, title, loader and not-found arrive with NEWFRONT-700.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { ArchivesDetailFrame } from "../../../../../../../features/archives/index.js";

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/archives/issues/$archivedIssueId")({
  component: ArchivedIssueDetail,
});

function ArchivedIssueDetail(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  return <ArchivesDetailFrame workspaceSlug={ws} projectId={projectId} />;
}
