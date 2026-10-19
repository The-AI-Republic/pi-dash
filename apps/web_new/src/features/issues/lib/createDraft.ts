// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Quick-create draft (parity/specs/shell.md §4, SHELL-047). The
// creation dialog keeps the unfinished title and project per
// workspace; a successful submit clears it. Corrupt or foreign
// payloads read as no draft.

import { loadPreference, removePreference, storePreference } from "../../../shared/shell/preferenceStorage.js";

/** Unfinished quick-create form: the title plus the picked project. */
export interface WorkItemDraft {
  title: string;
  projectId: string | null;
}

/** Profile key for one workspace's unfinished quick-create form. */
export function workItemDraftKey(workspaceSlug: string): string {
  return `pidash:work-item-draft:${workspaceSlug}`;
}

function isWorkItemDraft(value: unknown): value is WorkItemDraft {
  if (typeof value !== "object" || value === null) return false;
  const draft = value as Record<string, unknown>;
  return typeof draft["title"] === "string" && (draft["projectId"] === null || typeof draft["projectId"] === "string");
}

/** The unfinished form, or null when none was kept. */
export function loadWorkItemDraft(workspaceSlug: string): WorkItemDraft | null {
  return loadPreference(workItemDraftKey(workspaceSlug), null, isWorkItemDraft);
}

/** Keep the unfinished form for the next open. */
export function storeWorkItemDraft(workspaceSlug: string, draft: WorkItemDraft): void {
  storePreference(workItemDraftKey(workspaceSlug), draft);
}

/** Drop the kept form after a successful submit. */
export function clearWorkItemDraft(workspaceSlug: string): void {
  removePreference(workItemDraftKey(workspaceSlug));
}
