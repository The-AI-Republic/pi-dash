// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Mount point for a tab whose screen belongs to a later slice (PSET-001).
// The address deep-links, honors the role gate and keeps the rail; the
// surrounding screen owns the page header, and the tab's own slice
// replaces this body when it lands.

import * as React from "react";

export interface ProjectSettingsTabMountProps {
  /** Tab title, also the page heading. */
  title: string;
  /** Slice that fills in the tab's own screen. */
  owner: string;
}

export function ProjectSettingsTabMount({ title, owner }: ProjectSettingsTabMountProps): React.ReactElement {
  return (
    <section
      aria-label={`${title} settings`}
      data-testid="project-settings-tab-mount"
      data-slice={owner}
      className="flex flex-col gap-(--space-2)"
    >
      <p className="text-body text-(--text-muted)">This tab’s screen arrives with its own slice.</p>
    </section>
  );
}
