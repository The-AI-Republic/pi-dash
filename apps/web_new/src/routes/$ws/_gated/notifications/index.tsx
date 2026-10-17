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

import { InboxView } from "../../../../features/notifications/index.js";

export const Route = createFileRoute("/$ws/_gated/notifications/")({
  component: NotificationsInboxRoute,
});

function NotificationsInboxRoute(): React.ReactElement {
  const { ws } = Route.useParams();
  return <InboxView key={ws} workspaceSlug={ws} />;
}
