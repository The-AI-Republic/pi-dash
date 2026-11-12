// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Home query keys (Data layer). Every key starts with the workspace
// scope so a workspace switch or sign-out clears one subtree.

import { workspaceScope } from "../../../core/query/client.js";

export const homeKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "home"] as const,
};
