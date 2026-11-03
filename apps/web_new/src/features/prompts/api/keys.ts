// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Prompt query keys (Data layer). Every key starts with the workspace
// scope; sections, compiled receipts and previews each hang off their own
// subtree so a save invalidates the reads that reflect it.

import { workspaceScope } from "../../../core/query/client.js";

import type { PromptKind } from "../lib/sections.js";

export type PromptSectionScope = "user" | "workspace";

export const promptSectionKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "prompts", "sections"] as const,
  list: (workspaceSlug: string, kind: PromptKind, scope: PromptSectionScope) =>
    [...promptSectionKeys.all(workspaceSlug), kind, scope] as const,
};

export const promptReceiptKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "prompts", "receipts"] as const,
  compiled: (workspaceSlug: string, kind: PromptKind) => [...promptReceiptKeys.all(workspaceSlug), kind] as const,
};
