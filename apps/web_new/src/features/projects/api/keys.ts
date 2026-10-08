// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project query keys (Data layer). Every key starts with the workspace
// scope so a workspace switch or sign-out clears one subtree.

import { workspaceScope } from "../../../core/query/client.js";

export const projectKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "projects"] as const,
  list: (workspaceSlug: string) => [...projectKeys.all(workspaceSlug), "list"] as const,
};
