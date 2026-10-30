// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace-not-found screen (AUTH-024 Case A, from parity/specs/auth.md).
// The slug is absent from the workspace list, so the screen names the
// signed-in account, explains the address may not exist or may be out of
// reach, and offers a way home plus a profile visit when the user belongs
// somewhere, or workspace creation when they belong nowhere.

import { Link, useNavigate } from "@tanstack/react-router";
import { IconButton } from "@pidash/kit";
import { LogOut } from "lucide-react";
import * as React from "react";

import { accountSettingsGeneralHref } from "../../settings/index.js";
import { CrossAppLink } from "../../../shared/coexistence/index.js";
import { useSignOut } from "../signOut.js";
import { runSignOutAction } from "../commands.js";
import { useFocusOnMount } from "../focus.js";

export interface WorkspaceNotFoundScreenProps {
  /** The signed-in address shown in the header. */
  email: string;
  /**
   * A workspace the reader belongs to (main#670): it hosts the profile
   * link, because account settings live inside a workspace. Null when the
   * reader belongs to none, and then creation replaces both actions.
   */
  workspaceSlug: string | null;
}

export function WorkspaceNotFoundScreen({ email, workspaceSlug }: WorkspaceNotFoundScreenProps): React.ReactElement {
  const navigate = useNavigate();
  const signOut = useSignOut();
  const focusPrimary = useFocusOnMount<HTMLAnchorElement>();

  function handleSignOut(): void {
    void runSignOutAction(
      () => signOut.mutateAsync(),
      (landing) => {
        void navigate({ href: landing });
      }
    );
  }

  return (
    <div className="flex min-h-screen flex-col bg-(--bg)">
      <header className="flex items-center justify-between gap-(--space-4) p-(--space-6)">
        <span className="text-emphasis text-(--text)">Pi Dash</span>
        <div className="flex items-center gap-(--space-2)">
          <span className="text-body text-(--text-muted)">{email}</span>
          <IconButton label="Sign out" size="small" onClick={handleSignOut}>
            <LogOut size={14} aria-hidden="true" />
          </IconButton>
        </div>
      </header>
      <main className="flex flex-1 flex-col items-center justify-center gap-(--space-2) p-(--space-8) text-center">
        <img src="/illustrations/not-found.svg" alt="A workspace just out of reach" width={320} height={240} />
        <h1 className="text-h2 text-(--text)">Workspace not found</h1>
        <p className="text-body max-w-md text-(--text-muted)">
          This address may not exist, or it may be out of your reach.
        </p>
        <div className="flex items-center justify-center gap-(--space-2) pt-(--space-4)">
          {workspaceSlug !== null ? (
            <>
              <Link
                to="/"
                ref={focusPrimary}
                className="text-body rounded-(--radius-control) bg-(--accent) px-(--space-4) py-(--space-2) text-(--on-accent)"
              >
                Go Home
              </Link>
              <CrossAppLink
                to={accountSettingsGeneralHref(workspaceSlug)}
                className="text-body rounded-(--radius-control) border border-(--border) px-(--space-4) py-(--space-2) text-(--text)"
              >
                Open profile
              </CrossAppLink>
            </>
          ) : (
            <CrossAppLink
              to="/create-workspace"
              ref={focusPrimary}
              className="text-body rounded-(--radius-control) bg-(--accent) px-(--space-4) py-(--space-2) text-(--on-accent)"
            >
              Create a workspace
            </CrossAppLink>
          )}
        </div>
      </main>
    </div>
  );
}
