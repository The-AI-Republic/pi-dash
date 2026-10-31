// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// First-workspace popup (ADM-012, from parity/specs/admin.md §1.12):
// the dashboard layout mounts this above page content. The first
// instance-info load reporting no workspaces raises it; the create
// shortcut navigates without clearing it, and dismissing clears it for
// the page lifetime — a later truthful load never re-raises it. The
// create label is pinned by the parity scenario; the surrounding copy
// is fresh. (Mounted by NEWFRONT-506's dashboard layout.)
import { Link } from "@tanstack/react-router";
import { X } from "lucide-react";
import * as React from "react";

import { useInstanceBootstrap } from "../hooks.js";
import { useFirstWorkspaceStore } from "../stores.js";

/** Popup shortcut target: the create-workspace page (NEWFRONT-513). */
export const FIRST_WORKSPACE_CREATE_PATH = "/god-mode/workspace/create/";

type RouterLinkProps = Parameters<typeof Link>[0];

// The create page is not in the route tree yet, so the typed `to`
// cannot name it; the cast carries the literal through the same way
// CrossAppLink reaches out-of-tree paths.
const createLinkProps = { to: FIRST_WORKSPACE_CREATE_PATH } as RouterLinkProps;

export function FirstWorkspacePopup(): React.ReactElement | null {
  const bootstrap = useInstanceBootstrap();
  const dismissed = useFirstWorkspaceStore((state) => state.dismissed);
  const dismiss = useFirstWorkspaceStore((state) => state.dismiss);
  if (bootstrap.data?.instance.workspaces_exist !== false || dismissed) return null;
  return (
    <aside
      data-testid="admin-first-workspace-popup"
      className="shadow-lg fixed right-(--space-6) bottom-(--space-6) z-50 flex w-80 flex-col gap-(--space-3) rounded-(--radius-card) border border-(--border) bg-(--surface) p-(--space-4)"
    >
      <div className="flex items-start justify-between gap-(--space-2)">
        <p className="text-body font-medium text-(--text)">Create your first workspace</p>
        <button
          type="button"
          aria-label="Close"
          onClick={dismiss}
          className="cursor-pointer text-(--text-muted) hover:text-(--text)"
        >
          <X size={16} aria-hidden="true" />
        </button>
      </div>
      <p className="text-body text-(--text-muted)">
        This instance has no workspaces yet. Create one to start inviting people.
      </p>
      <Link
        {...createLinkProps}
        className="text-body rounded-(--radius-control) bg-(--accent) px-(--space-4) py-(--space-2) text-center text-(--on-accent)"
      >
        Create workspace
      </Link>
    </aside>
  );
}
