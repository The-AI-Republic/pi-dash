// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Profile query keys (parity/specs/profile.md §1–4). Each key starts under
// the workspace scope so a workspace switch clears the whole subtree; the
// viewed member id and the ownership selector keep the three issue tabs
// apart.

import { workspaceScope } from "../../../core/query/client.js";
import type { ProfileOwnership } from "@pidash/api-client";

export const profileKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "profile"] as const,
  segregation: (workspaceSlug: string, userId: string) =>
    [...profileKeys.all(workspaceSlug), "segregation", userId] as const,
  stats: (workspaceSlug: string, userId: string) => [...profileKeys.all(workspaceSlug), "stats", userId] as const,
  issues: (workspaceSlug: string, userId: string, ownership: ProfileOwnership) =>
    [...profileKeys.all(workspaceSlug), "issues", userId, ownership] as const,
  activity: (workspaceSlug: string, userId: string, perPage: number) =>
    [...profileKeys.all(workspaceSlug), "activity", userId, perPage] as const,
};
