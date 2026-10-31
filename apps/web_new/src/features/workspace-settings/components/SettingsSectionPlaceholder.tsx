// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Quiet section placeholder (WSET-001/036). The shell owns navigation,
// gate and addresses; a slice that has not registered its body yet
// renders this honest state inside the frame instead of a fake page.
// There are no controls, file inputs, drag targets or shortcuts here.

import { PanelsTopLeft } from "lucide-react";
import * as React from "react";

export const SETTINGS_SECTION_PLACEHOLDER_TITLE = "This settings page is not available yet";

export function SettingsSectionPlaceholder(): React.ReactElement {
  return (
    <section
      data-testid="workspace-settings-section-placeholder"
      className="flex flex-col items-center gap-(--space-3) rounded-(--radius-control) border border-dashed border-(--border) px-(--space-6) py-(--space-12) text-center"
    >
      <PanelsTopLeft size={28} strokeWidth={1.25} aria-hidden="true" className="text-(--text-muted)" />
      <h2 className="text-h3 text-(--text)">{SETTINGS_SECTION_PLACEHOLDER_TITLE}</h2>
      <p className="text-body max-w-sm text-(--text-muted)">
        This section is being connected to the workspace settings frame.
      </p>
    </section>
  );
}
