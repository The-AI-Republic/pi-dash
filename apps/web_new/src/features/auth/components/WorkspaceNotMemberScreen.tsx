// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Not-a-member screen (AUTH-024 Case B, from parity/specs/auth.md). The
// slug is known in the workspace list but no membership info exists, so
// the screen conveys the missing membership and points at pending
// invitations and workspace creation.

import * as React from "react";

import { CrossAppLink } from "../../../shared/coexistence/index.js";
import { useFocusOnMount } from "../focus.js";

export function WorkspaceNotMemberScreen(): React.ReactElement {
  const focusPrimary = useFocusOnMount<HTMLAnchorElement>();
  return (
    <main className="flex min-h-screen flex-col items-center justify-center gap-(--space-2) bg-(--bg) p-(--space-8) text-center">
      <h1 className="text-h2 text-(--text)">Not Authorized!</h1>
      <p className="text-body max-w-md text-(--text-muted)">
        You&apos;re not a member of this workspace. Please contact the workspace admin to get an invitation or check
        your pending invitations.
      </p>
      <div className="flex items-center justify-center gap-(--space-2) pt-(--space-4)">
        <CrossAppLink
          to="/invitations"
          ref={focusPrimary}
          className="text-body rounded-(--radius-control) bg-(--accent) px-(--space-4) py-(--space-2) text-(--on-accent)"
        >
          View pending invitations
        </CrossAppLink>
        <CrossAppLink
          to="/create-workspace"
          className="text-body rounded-(--radius-control) border border-(--border) px-(--space-4) py-(--space-2) text-(--text)"
        >
          Create a workspace
        </CrossAppLink>
      </div>
    </main>
  );
}
