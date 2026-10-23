// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake two-pane frame (parity/specs/intake.md INT-001). The trail and
// header sit above a body split into the list pane and the content pane.
// With no request selected the content pane shows the pick placeholder;
// the detail itself is slice D, so any other selection keeps the
// placeholder until that slice lands. The frame owns layout only; the
// screen owns reads and the address.

import * as React from "react";

import { intakeHref } from "../lib/paths.js";
import { INTAKE_AREA_LABEL } from "../lib/title.js";
import type { IntakeTab } from "../lib/tabs.js";
import { IntakeBreadcrumb } from "./IntakeBreadcrumb.js";
import { IntakeListPane } from "./IntakeListPane.js";
import { IntakePlaceholder } from "./IntakePlaceholder.js";

export interface IntakeShellProps {
  workspaceSlug: string;
  workspaceName: string;
  projectId: string;
  projectName: string;
  tab: IntakeTab;
  openTotal: number | undefined;
  loading: boolean;
  failed: boolean;
  rows: React.ComponentProps<typeof IntakeListPane>["rows"];
  emptyKind: React.ComponentProps<typeof IntakeListPane>["emptyKind"];
  selectedId: string | undefined;
  onSelectTab: (tab: IntakeTab) => void;
  onSelectRow: (issueId: string) => void;
  onRetry: () => void;
  onCreate?: (() => void) | undefined;
}

export function IntakeShell({
  workspaceSlug,
  workspaceName,
  projectId,
  projectName,
  tab,
  openTotal,
  loading,
  failed,
  rows,
  emptyKind,
  selectedId,
  onSelectTab,
  onSelectRow,
  onRetry,
  onCreate,
}: IntakeShellProps): React.ReactElement {
  return (
    <section
      aria-label={INTAKE_AREA_LABEL}
      data-testid="intake-shell"
      className="flex min-h-0 flex-1 flex-col bg-(--bg) text-(--text)"
    >
      <header className="flex shrink-0 flex-col gap-(--space-2) px-(--space-4) py-(--space-3)">
        <IntakeBreadcrumb
          workspaceName={workspaceName}
          projectName={projectName}
          href={intakeHref(workspaceSlug, projectId)}
        />
      </header>
      <div className="flex min-h-0 flex-1">
        <IntakeListPane
          tab={tab}
          openTotal={openTotal}
          loading={loading}
          failed={failed}
          rows={rows}
          emptyKind={emptyKind}
          selectedId={selectedId}
          onSelectTab={onSelectTab}
          onSelectRow={onSelectRow}
          onRetry={onRetry}
          onCreate={onCreate}
        />
        <div data-testid="intake-content-pane" className="min-h-0 min-w-0 flex-1">
          {selectedId === undefined ? <IntakePlaceholder /> : null}
        </div>
      </div>
    </section>
  );
}
