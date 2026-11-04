// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Content frame inside the settings shell (parity/specs/workspace-settings.md
// §1, WSET-001). The frame owns the section heading, a short section
// description slot and a scrolling body. Page slices register their body
// through the registry and render inside it. The heading and the body
// share one centred max-width column; wide pages opt out with fullWidth.

import * as React from "react";

export interface SettingsPageFrameProps {
  title: string;
  description?: string | undefined;
  /** Wide pages (roster tables, plan grids) skip the centred column. */
  fullWidth?: boolean | undefined;
  children: React.ReactNode;
}

/** Centred column shared by the header content and the body. */
const CONTENT_COLUMN = "mx-auto w-full max-w-(--settings-content-max)";

export function SettingsPageFrame({
  title,
  description,
  fullWidth = false,
  children,
}: SettingsPageFrameProps): React.ReactElement {
  const heading = (
    <>
      <nav aria-label="Breadcrumb" className="mb-(--space-1) hidden md:block">
        <div className="text-meta relative line-clamp-1 text-(--text-muted)">{title}</div>
      </nav>
      <h1 className="text-h2 text-(--text)">{title}</h1>
      {description ? <p className="text-body mt-(--space-1) text-(--text-muted)">{description}</p> : null}
    </>
  );
  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
      <header className="border-b border-(--border) px-(--space-6) py-(--space-4)">
        {fullWidth ? (
          heading
        ) : (
          <div data-testid="settings-page-header-content" className={CONTENT_COLUMN}>
            {heading}
          </div>
        )}
      </header>
      <div className="min-h-0 flex-1 px-(--space-6) py-(--space-6)">
        {fullWidth ? (
          children
        ) : (
          <div data-testid="settings-page-content" className={CONTENT_COLUMN}>
            {children}
          </div>
        )}
      </div>
    </div>
  );
}
