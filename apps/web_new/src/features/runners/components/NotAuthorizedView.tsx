// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Area not-authorized view (NEWFRONT-540, RUN-001). The oracle pins this
// heading, so the copy is part of the parity contract: it renders inside
// the normal workspace or project chrome, never as a bare page.

import * as React from "react";

export const RUNNERS_DENIED_HEADING = "Oops! You are not authorized to view this page";

export interface RunnersNotAuthorizedProps {
  /** Which scope denied the viewer, for the supporting sentence. */
  scope: "workspace" | "project";
}

export function RunnersNotAuthorized({ scope }: RunnersNotAuthorizedProps): React.ReactElement {
  return (
    <section
      aria-label="Not authorized"
      data-testid="runners-denied"
      className="flex h-full flex-col items-center justify-center gap-(--space-2) p-(--space-12) text-center"
    >
      <h2 className="text-h3 text-(--text)">{RUNNERS_DENIED_HEADING}</h2>
      <p className="text-body max-w-md text-(--text-muted)">
        {scope === "project"
          ? "Only admins and members of this project can open its AI runners."
          : "Only workspace admins and members can open AI runners."}
      </p>
    </section>
  );
}
