// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Prompt section query keys (Data layer). Every key starts with the
// workspace scope; one list per kind and scope, so a save invalidates the
// whole sections subtree while a kind never collides with another.

import { workspaceScope } from "../../../core/query/client.js";

import type { PromptKind } from "../lib/sections.js";

export type PromptSectionScope = "user" | "workspace";

export const promptSectionKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "prompts", "sections"] as const,
  list: (workspaceSlug: string, kind: PromptKind, scope: PromptSectionScope) =>
    [...promptSectionKeys.all(workspaceSlug), kind, scope] as const,
};
