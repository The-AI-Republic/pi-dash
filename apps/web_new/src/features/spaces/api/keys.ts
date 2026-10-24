// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Every public-board key is rooted at ["spaces", ...], never ["ws", ...],
// so a workspace switch or sign-out cannot clear a visitor's board.

export const spaceKeys = {
  all: ["spaces"] as const,
  instance: ["spaces", "instance"] as const,
  anchor: (anchor: string) => ["spaces", "anchor", anchor] as const,
  settings: (anchor: string) => ["spaces", "anchor", anchor, "settings"] as const,
  issues: (anchor: string, filterSignature: string) => ["spaces", "anchor", anchor, "issues", filterSignature] as const,
  /** One workflow state's later pages; the layout is deliberately absent so both reuse them. */
  group: (anchor: string, stateId: string, filterSignature: string) =>
    ["spaces", "anchor", anchor, "issues", filterSignature, "group", stateId] as const,
  states: (anchor: string) => ["spaces", "anchor", anchor, "states"] as const,
  meta: (anchor: string) => ["spaces", "anchor", anchor, "meta"] as const,
  legacy: (workspaceSlug: string, projectId: string) => ["spaces", "legacy", workspaceSlug, projectId] as const,
} as const;
