// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Console route guards (ADM-009, from parity/specs/admin.md §1.9). The
// home guard wraps the entry tree (the OSS gate and, in cloud, the
// resolver NEWFRONT-519 mounts) and replace-navigates a signed-in admin
// to general settings; the dashboard guard wraps the dashboard tree
// (NEWFRONT-506's layout and every page under it) and replace-navigates
// anyone else to the root. Both show the brand loader while the admin
// check is unresolved, so the guarded screen never flashes first, and
// both redirect with replace, adding no history entry. The catch-all
// route renders outside both guards.
import { Navigate } from "@tanstack/react-router";
import * as React from "react";

import { decideAdminGuard, type AdminGuardDecision, type AdminGuardTree } from "../adminGuards.js";
import { useAdminSession } from "../hooks.js";
import { AdminBrandLoader } from "./AdminBrandLoader.js";

function useGuardDecision(tree: AdminGuardTree): AdminGuardDecision | null {
  const session = useAdminSession();
  if (session.isPending) return null;
  return decideAdminGuard({ tree, session: session.data ?? null });
}

function Guarded({ tree, children }: { tree: AdminGuardTree; children: React.ReactNode }): React.ReactElement {
  const decision = useGuardDecision(tree);
  if (decision === null) return <AdminBrandLoader />;
  // Navigate renders nothing and commits on the next beat, so no
  // guarded content shows before the replace lands.
  if (decision.action === "redirect") return <Navigate to={decision.target} replace />;
  return <>{children}</>;
}

export function AdminHomeGuard({ children }: { children: React.ReactNode }): React.ReactElement {
  return <Guarded tree="home">{children}</Guarded>;
}

export function AdminDashboardGuard({ children }: { children: React.ReactNode }): React.ReactElement {
  return <Guarded tree="dashboard">{children}</Guarded>;
}
