// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Scoped analytics dialog (parity/specs/analytics.md §13, ANL-014; §19,
// ANL-018). A right-hand panel over one project, optionally narrowed to a
// cycle or module. The opening surface owns the trigger; this file owns
// the content and the panel's own life: it spins while the scope's first
// reads are in flight, renders the five work-items cards, created-vs-
// resolved, custom insights with their table, then the assignee
// breakdown, and lets the caller widen it on medium screens and up.
// Nothing here gates a role: analytics is read-only for every member,
// guests included, and each trigger's owner decides who sees an entry.
import { Dialog, IconButton, Spinner } from "@pidash/kit";
import { Maximize2, Minimize2 } from "lucide-react";
import * as React from "react";

import type { AnalyticsDialogScope } from "../api/keys.js";
import { useDialogStats, useDialogTotals } from "../api/projectScope.js";
import { scopedDialogTitle } from "../lib/dialogTitle.js";
import { AssigneeBreakdownSection } from "./AssigneeBreakdownSection.js";
import { CreatedResolvedSection } from "./CreatedResolvedSection.js";
import { CustomInsightsSection } from "./CustomInsightsSection.js";
import { HeadlineCards } from "./HeadlineCards.js";

export interface ScopedAnalyticsDialogProps {
  workspaceSlug: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Project the panel reads; the path id and `project_ids` both come from it. */
  projectId: string;
  projectName: string;
  cycle?: { id: string; name: string } | undefined;
  module?: { id: string; name: string } | undefined;
}

/** The wide toggle is a medium-and-up control; narrow screens never show it. */
const WIDE_TOGGLE_CLASS = "hidden md:inline-flex";

/**
 * The scope the panel reads: project first, then the cycle or module
 * narrowing. A caller that passes both narrows by cycle, which is the
 * order the server applies.
 */
export function dialogScopeFor(
  projectId: string,
  cycle: { id: string } | undefined,
  module: { id: string } | undefined
): AnalyticsDialogScope {
  return {
    projectId,
    cycleId: cycle?.id ?? null,
    moduleId: cycle === undefined ? (module?.id ?? null) : null,
  };
}

export function ScopedAnalyticsDialog({
  workspaceSlug,
  open,
  onOpenChange,
  projectId,
  projectName,
  cycle,
  module,
}: ScopedAnalyticsDialogProps): React.ReactElement {
  const [wide, setWide] = React.useState(false);
  const panel = React.useRef<HTMLDivElement>(null);
  // Only the ids decide the read; the names just label the panel. The
  // query keys serialize the scope, so an inline object is stable enough
  // for the cache while an id change still starts a fresh read.
  const scope = dialogScopeFor(projectId, cycle, module);
  const title = scopedDialogTitle({
    projectName,
    cycleName: cycle?.name ?? null,
    moduleName: module?.name ?? null,
  });
  // The panel waits for the scope's first two reads before it shows any
  // content, so a scope change never paints another scope's numbers.
  // A closed panel costs nothing: the header entry sits on every issue
  // list visit, so its reads wait for the open.
  const totals = useDialogTotals(workspaceSlug, scope, open);
  const stats = useDialogStats(workspaceSlug, scope, open);
  const settling = open && (totals.isPending || stats.isPending);

  const handleOpenChange = React.useCallback(
    (next: boolean) => {
      // Closing drops the panel's own view state; the workspace page is a
      // separate surface and keeps its picker selection untouched.
      if (!next) setWide(false);
      onOpenChange(next);
    },
    [onOpenChange]
  );

  return (
    <Dialog
      open={open}
      onOpenChange={handleOpenChange}
      title={title}
      placement="right"
      wide={wide}
      popupClassName="min-w-0"
      initialFocus={panel}
      headerActions={
        <span className={WIDE_TOGGLE_CLASS}>
          <IconButton
            label={wide ? "Use the narrower panel" : "Widen the panel"}
            data-testid="analytics-dialog-wide-toggle"
            onClick={() => setWide(!wide)}
          >
            {wide ? (
              <Minimize2 aria-hidden="true" className="size-4" />
            ) : (
              <Maximize2 aria-hidden="true" className="size-4" />
            )}
          </IconButton>
        </span>
      }
    >
      <div
        ref={panel}
        tabIndex={-1}
        data-testid="analytics-dialog-panel"
        className="flex flex-col gap-(--space-6) outline-none"
      >
        <div data-testid="issues-analytics-body" className="flex flex-col gap-(--space-6)">
          {settling ? (
            <div className="flex items-center justify-center py-(--space-12)">
              <Spinner label="Loading analytics" />
            </div>
          ) : (
            <>
              <HeadlineCards workspaceSlug={workspaceSlug} tab="work-items" projectIds={[]} dialogScope={scope} />
              <CreatedResolvedSection workspaceSlug={workspaceSlug} projectIds={[]} dialogScope={scope} />
              <CustomInsightsSection workspaceSlug={workspaceSlug} projectIds={[]} dialogScope={scope} />
              <AssigneeBreakdownSection workspaceSlug={workspaceSlug} scope={scope} />
            </>
          )}
        </div>
      </div>
    </Dialog>
  );
}
