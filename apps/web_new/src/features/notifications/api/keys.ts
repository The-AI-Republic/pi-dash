// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Notification query keys (Data layer). Workspace-scoped keys start with
// the workspace scope so a workspace switch or sign-out clears one
// subtree. The email preferences are the caller's own settings — the same
// in every workspace — so they hang off a user-level root instead
// (sign-out clears the whole cache, so no stale user survives). The shell
// reads only the unread count through these; fetching semantics,
// fixtures and inbox behavior belong to the notifications area, which
// extends this module.
import { workspaceScope } from "../../../core/query/client.js";

export const notificationKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "notifications"] as const,
  unread: (workspaceSlug: string) => [...notificationKeys.all(workspaceSlug), "unread"] as const,
  /** The caller's email delivery preferences (user-level, no workspace). */
  emailPreferences: ["notifications", "email-preferences"] as const,
};
