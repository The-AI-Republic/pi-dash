// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake cache keys (parity/specs/intake.md, Data layer). Every key sits
// under the shared workspace root ["ws", slug], so a workspace switch or
// sign-out clears the whole subtree with the rest of the workspace data.
// The selection is deliberately absent from every key: picking a request
// must not refetch the list.

import { workspaceScope } from "../../../core/query/client.js";
import type { IntakeStatus } from "@pidash/api-client";

import type { IntakeTab } from "../lib/tabs.js";

/** Ordering fields the list accepts (INT-011); slice B adds the menu. */
export const INTAKE_DEFAULT_ORDER = "-issue__created_at";

/** The list read's filter set; the tab's status set is always present. */
export interface IntakeListFilters {
  statuses: readonly IntakeStatus[];
  priorities?: readonly string[];
  assignees?: readonly string[];
  createdBy?: readonly string[];
  labels?: readonly string[];
  createdAt?: string;
  updatedAt?: string;
}

export interface IntakeListKeyInput {
  tab: IntakeTab;
  filters: IntakeListFilters;
  orderBy: string;
}

export const intakeKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "intake"] as const,
  lists: (workspaceSlug: string, projectId: string) => [...intakeKeys.all(workspaceSlug), "list", projectId] as const,
  list: (workspaceSlug: string, projectId: string, input: IntakeListKeyInput) =>
    [
      ...intakeKeys.lists(workspaceSlug, projectId),
      {
        tab: input.tab,
        statuses: [...input.filters.statuses],
        priorities: [...(input.filters.priorities ?? [])],
        assignees: [...(input.filters.assignees ?? [])],
        createdBy: [...(input.filters.createdBy ?? [])],
        labels: [...(input.filters.labels ?? [])],
        createdAt: input.filters.createdAt ?? null,
        updatedAt: input.filters.updatedAt ?? null,
        orderBy: input.orderBy,
      },
    ] as const,
  details: (workspaceSlug: string, projectId: string) =>
    [...intakeKeys.all(workspaceSlug), "detail", projectId] as const,
  detail: (workspaceSlug: string, projectId: string, issueId: string) =>
    [...intakeKeys.details(workspaceSlug, projectId), issueId] as const,
};
