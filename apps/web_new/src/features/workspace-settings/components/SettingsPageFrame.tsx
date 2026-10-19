// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Content frame inside the settings shell (parity/specs/workspace-settings.md
// §1, WSET-001). The frame owns the section heading, a short section
// description slot and a scrolling body. Page slices register their body
// through the registry and render inside it.

import * as React from "react";

export interface SettingsPageFrameProps {
  title: string;
  description?: string | undefined;
  children: React.ReactNode;
}

export function SettingsPageFrame({ title, description, children }: SettingsPageFrameProps): React.ReactElement {
  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
      <header className="border-b border-(--border) px-(--space-6) py-(--space-4)">
        <h1 className="text-h2 text-(--text)">{title}</h1>
        {description ? <p className="text-body mt-(--space-1) text-(--text-muted)">{description}</p> : null}
      </header>
      <div className="min-h-0 flex-1 px-(--space-6) py-(--space-6)">{children}</div>
    </div>
  );
}
