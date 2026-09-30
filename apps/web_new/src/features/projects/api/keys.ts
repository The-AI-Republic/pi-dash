// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Project query keys (Data layer). Every key starts with the workspace
// scope so a workspace switch or sign-out clears one subtree.

import { workspaceScope } from "../../../core/query/client.js";

export const projectKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "projects"] as const,
  list: (workspaceSlug: string) => [...projectKeys.all(workspaceSlug), "list"] as const,
};
