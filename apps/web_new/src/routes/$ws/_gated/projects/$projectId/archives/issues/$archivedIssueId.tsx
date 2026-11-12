// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archived-issue detail route (parity/specs/archives.md §3.4–3.5,
// ARCH-011–012). Thin: the archived address mounts the locked detail
// screen, which owns the banner, loader, not-found state, short-key
// crumb and quick actions inside the archives frame.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { ArchivedIssueDetail } from "../../../../../../../features/issues/contexts/ArchivedIssueDetail.js";

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/archives/issues/$archivedIssueId")({
  component: ArchivedIssueDetailRoute,
});

function ArchivedIssueDetailRoute(): React.ReactElement {
  const { ws, projectId, archivedIssueId } = Route.useParams();
  return <ArchivedIssueDetail workspaceSlug={ws} projectId={projectId} issueId={archivedIssueId} />;
}
