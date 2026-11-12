// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Inline denial for the three issue-tab addresses a guest opens directly
// (parity/specs/profile.md §1, PROF-002). The frame stays mounted around
// it: the strip, picker and sidebar keep rendering, only the tab body is
// replaced by the notice.

import * as React from "react";

export const PROFILE_DENIED_TITLE = "You do not have permission to view this profile tab";
export const PROFILE_DENIED_DETAIL = "Ask a workspace admin for access to this member's work items.";

export function ProfileAccessDenied(): React.ReactElement {
  return (
    <section
      aria-label="Profile access denied"
      data-testid="profile-access-denied"
      className="flex flex-col items-center gap-(--space-2) p-(--space-12) text-center"
    >
      <h2 className="text-h3 text-(--text)">{PROFILE_DENIED_TITLE}</h2>
      <p className="text-body text-(--text-muted)">{PROFILE_DENIED_DETAIL}</p>
    </section>
  );
}
