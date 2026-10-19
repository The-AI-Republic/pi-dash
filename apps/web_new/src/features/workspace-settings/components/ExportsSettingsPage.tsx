// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Exports settings entry (parity/specs/workspace-settings.md §33,
// WSET-033). Admittance and the tab title belong to the settings shell
// (admin/member allowed, guest denied); this page explains the purpose
// and gives the exports area one named slot for its guide. It renders no
// export form, history, download or polling of its own.

import { FileDown } from "lucide-react";
import * as React from "react";

import { useWorkspaceExportsGuide } from "../lib/exportsGuide.js";

export const EXPORTS_PAGE_TITLE = "Exports";
export const EXPORTS_PAGE_DESCRIPTION =
  "Export your project data in various formats and access your export history with download links.";
export const EXPORTS_GUIDE_PENDING_TITLE = "Export guide unavailable";

export interface ExportsSettingsPageProps {
  workspaceSlug: string;
}

function ExportsGuidePending(): React.ReactElement {
  return (
    <section
      data-testid="workspace-exports-guide-pending"
      className="flex flex-col items-center gap-(--space-3) rounded-(--radius-control) border border-dashed border-(--border) px-(--space-6) py-(--space-12) text-center"
    >
      <FileDown size={28} strokeWidth={1.25} aria-hidden="true" className="text-(--text-muted)" />
      <h2 className="text-h3 text-(--text)">{EXPORTS_GUIDE_PENDING_TITLE}</h2>
      <p className="text-body max-w-sm text-(--text-muted)">
        The export form and previous exports are being connected to this entry.
      </p>
    </section>
  );
}

export function ExportsSettingsPage({ workspaceSlug }: ExportsSettingsPageProps): React.ReactElement {
  const Guide = useWorkspaceExportsGuide();
  return (
    <section data-testid="workspace-exports-settings" className="flex flex-col gap-(--space-6)">
      <div className="flex items-start gap-(--space-3)">
        <span className="flex size-10 shrink-0 items-center justify-center rounded-(--radius-control) bg-(--subtle) text-(--accent)">
          <FileDown size={20} strokeWidth={1.5} aria-hidden="true" />
        </span>
        <div className="flex flex-col gap-(--space-1)">
          <h2 className="text-h3 text-(--text)">{EXPORTS_PAGE_TITLE}</h2>
          <p className="text-body max-w-2xl text-(--text-muted)">{EXPORTS_PAGE_DESCRIPTION}</p>
        </div>
      </div>
      {Guide === null ? <ExportsGuidePending /> : <Guide workspaceSlug={workspaceSlug} />}
    </section>
  );
}
