// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared frame for the three AI-assistant sections (parity/specs/profile.md
// §10). Each section is its own <section> with an h3 heading, a lead
// paragraph and a stacked form column; the heading text is how the parity
// driver tells the repeated Save/Test/Remove controls apart.

import * as React from "react";

export interface SectionFrameProps {
  heading: string;
  description: string;
  testId: string;
  /** Deep-link anchor for the section, when one links at it. */
  id?: string | undefined;
  children: React.ReactNode;
}

export function SectionFrame({ heading, description, testId, id, children }: SectionFrameProps): React.ReactElement {
  const headingId = `${testId}-heading`;
  return (
    <section aria-labelledby={headingId} data-testid={testId} id={id} className="flex flex-col gap-(--space-4)">
      <div className="flex flex-col gap-(--space-1)">
        <h3 id={headingId} className="text-emphasis font-medium text-(--text)">
          {heading}
        </h3>
        <p className="text-body text-(--text-muted)">{description}</p>
      </div>
      {children}
    </section>
  );
}

/** The action row every editable section ends with. */
export function ActionRow({ children }: { children: React.ReactNode }): React.ReactElement {
  return <div className="flex flex-wrap items-center gap-(--space-2)">{children}</div>;
}
