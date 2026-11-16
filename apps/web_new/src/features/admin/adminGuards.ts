// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Admin route-guard decider (ADM-009, from parity/specs/admin.md §1.9).
// Pure, so guards and tests share it: the home tree bounces a signed-in
// admin to general settings, the dashboard tree bounces anyone else to
// the root. Guards adapt decisions to replace navigations; the loader
// covers the unresolved check.
import type { AdminSession } from "@pidash/api-client";

import { GOD_MODE_ENTRY } from "./adminErrors.js";

/** Console landing: every guard redirect and the not-found shortcut target. */
export const GOD_MODE_GENERAL = "/god-mode/general/";

export type AdminGuardTree = "home" | "dashboard";

export type AdminGuardDecision = { action: "render" } | { action: "redirect"; target: string };

/**
 * Gate one console tree. `session` is null when the admin check fails
 * transiently (5xx, network): unreadable counts as anonymous, so the
 * home tree shows the entry and the dashboard tree bounces to it. A
 * revoked grant (forbidden) reads as anonymous on both trees.
 */
export function decideAdminGuard(args: { tree: AdminGuardTree; session: AdminSession | null }): AdminGuardDecision {
  const signedIn = args.session?.kind === "admin";
  if (args.tree === "home") {
    return signedIn ? { action: "redirect", target: GOD_MODE_GENERAL } : { action: "render" };
  }
  return signedIn ? { action: "render" } : { action: "redirect", target: GOD_MODE_ENTRY };
}
