// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar query keys (Data layer). Every key starts with the workspace
// scope so a workspace switch or sign-out clears one subtree.

import { workspaceScope } from "../../../core/query/client.js";

export const sidebarKeys = {
  /** The caller's sidebar preferences (pins and ordering). */
  preferences: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "sidebar-preferences"] as const,
  /** The caller's top-level favorites. */
  favorites: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "favorites"] as const,
  /** One favorites folder's children. */
  favoriteGroup: (workspaceSlug: string, folderId: string) =>
    [...workspaceScope(workspaceSlug), "favorites", folderId] as const,
};
