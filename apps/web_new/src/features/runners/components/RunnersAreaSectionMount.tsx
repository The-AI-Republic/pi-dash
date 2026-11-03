// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Child-screen mount point (NEWFRONT-540, RUN-003/RUN-004/RUN-010). The runs,
// approvals, detail and chat screens belong to later slices; until they land
// their addresses render the area name inside the shell so every tab and rail
// link resolves, deep-links and keeps the tab strip correct.

import * as React from "react";

export interface RunnersAreaSectionMountProps {
  /** Section name, also the heading. */
  title: string;
}

export function RunnersAreaSectionMount({ title }: RunnersAreaSectionMountProps): React.ReactElement {
  return (
    <section
      aria-label={`${title} section`}
      data-testid="runners-section-mount"
      className="flex flex-col gap-(--space-2) p-(--space-6)"
    >
      <h1 className="text-h2 text-(--text)">{title}</h1>
    </section>
  );
}
