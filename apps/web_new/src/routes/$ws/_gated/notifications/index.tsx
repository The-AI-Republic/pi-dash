// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace notifications inbox (NTF-001; written from
// parity/specs/notifications.md §1). Thin: the gated layout already gates
// the session and prefetches the unread totals behind the shell dot, so
// this resolves the workspace slug and mounts the connected inbox view.
// The key remounts per workspace so tab state never leaks across
// workspaces.
import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import type { InboxView as InboxViewComponent } from "../../../../features/notifications/components/InboxView.js";

// Deferred (size gate): route files are eager, so importing the inbox at
// module scope would pull its feature closure into the initial chunk.
const InboxView = React.lazy(() =>
  import("../../../../features/notifications/components/InboxView.js").then((module) => ({
    default: module.InboxView,
  }))
) as typeof InboxViewComponent;

export const Route = createFileRoute("/$ws/_gated/notifications/")({
  component: NotificationsInboxRoute,
});

function NotificationsInboxRoute(): React.ReactElement {
  const { ws } = Route.useParams();
  return (
    <React.Suspense fallback={null}>
      <InboxView key={ws} workspaceSlug={ws} />
    </React.Suspense>
  );
}
