// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Not-authorized surface (parity/specs/workspace-settings.md §1/§2,
// WSET-002/007). Rendered on the requested address instead of the
// shell: no sidebar, no content, and no page title. The copy names the
// reason, keeps the way back visible without redirecting, and leaves the
// tab title empty (§2/§36).

import { ShieldOff } from "lucide-react";
import * as React from "react";

import { useDocumentHead } from "../../../shared/shell/index.js";

export const SETTINGS_NOT_AUTHORIZED_HEADING = "You are not authorized to view this page";

export interface SettingsNotAuthorizedProps {
  workspaceSlug: string;
}

export function SettingsNotAuthorized({ workspaceSlug }: SettingsNotAuthorizedProps): React.ReactElement {
  useDocumentHead({ title: "", robots: "noindex, nofollow" });
  return (
    <main className="fixed inset-0 z-10 flex items-center justify-center bg-(--bg) px-(--space-6) py-(--space-12)">
      <section
        aria-labelledby="workspace-settings-denied-heading"
        data-testid="workspace-settings-not-authorized"
        className="flex w-full max-w-lg flex-col items-center gap-(--space-4) text-center"
      >
        <span className="flex size-16 items-center justify-center rounded-full bg-(--subtle) text-(--text-muted)">
          <ShieldOff size={28} strokeWidth={1.25} aria-hidden="true" />
        </span>
        <h1 id="workspace-settings-denied-heading" className="text-h2 text-(--text)">
          {SETTINGS_NOT_AUTHORIZED_HEADING}
        </h1>
        <p className="text-body text-(--text-muted)">
          This settings section is limited to another workspace role. Ask a workspace admin if you need access.
        </p>
        <a
          href={`/${workspaceSlug}`}
          className="text-body rounded-(--radius-control) border border-(--border) px-(--space-4) py-(--space-2) text-(--text) hover:bg-(--subtle)"
        >
          Back to workspace
        </a>
      </section>
    </main>
  );
}
