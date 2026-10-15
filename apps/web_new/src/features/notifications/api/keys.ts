// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Notification query keys (Data layer). Every key starts with the
// workspace scope so a workspace switch or sign-out clears one subtree.
// The shell reads only the unread count through these; fetching
// semantics, fixtures and inbox behavior belong to the notifications
// area, which extends this module.
import { workspaceScope } from "../../../core/query/client.js";

export const notificationKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "notifications"] as const,
  unread: (workspaceSlug: string) => [...notificationKeys.all(workspaceSlug), "unread"] as const,
};
