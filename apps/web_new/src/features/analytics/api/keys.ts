// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics query keys (parity/specs/analytics.md §16.6). Every key
// starts with the workspace scope so a workspace switch or sign-out
// clears one subtree; the picker selection is part of every key so
// each scope caches separately.

import { workspaceScope } from "../../../core/query/client.js";

import type { AnalyticsTabId } from "../lib/tabs.js";

export const analyticsKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "analytics"] as const,
  /** One tab's headline totals under one normalized project scope. */
  totals: (workspaceSlug: string, tab: AnalyticsTabId, projectIds: readonly string[]) =>
    [...analyticsKeys.all(workspaceSlug), "totals", tab, projectIds] as const,
};
