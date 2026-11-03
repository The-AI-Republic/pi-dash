// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The two built-in idle-automation rows
// (parity/specs/agents.md §8, AGT-034–036). The archive row pairs the
// switch with a delay picker; the close row adds the cancelled-state
// target. Both write through the page's single handler; a switch renders
// on only when its stored delay is positive, and the project row is
// always present by then (the page waits for it).

import type { ProjectDetails, State } from "@pidash/api-client";
import { PackageCheck, PackageMinus } from "lucide-react";
import * as React from "react";

import type { AutomationPatch } from "../api/automationQueries.js";
import { isDelayEnabled } from "../api/months.js";
import { archiveTogglePatch, closeTogglePatch, selectCancelledStates, selectCloseTarget } from "../lib/rows.js";
import { AutomationRow } from "./AutomationRow.js";
import { CloseTargetPicker } from "./CloseTargetPicker.js";
import { CustomMonthDialog } from "./CustomMonthDialog.js";
import { DelayPicker } from "./DelayPicker.js";

export const ARCHIVE_ROW_TITLE = "Auto-archive closed work items";
export const ARCHIVE_ROW_DESCRIPTION =
  "Closed items move to the project archive once the waiting period below has passed.";
export const ARCHIVE_DELAY_LEAD = "Archiving window";
export const CLOSE_ROW_TITLE = "Auto-close work items";
export const CLOSE_ROW_DESCRIPTION =
  "Inactive items are moved to a cancelled state after the waiting period below, keeping open lists current.";
export const CLOSE_DELAY_LEAD = "Closing window";
export const CLOSE_STATE_LEAD = "Cancelled state";

export interface AutomationRowsProps {
  /** The project row; undefined while it loads. */
  project: ProjectDetails | undefined;
  /** All project states; the close row filters the cancelled group. */
  states: readonly State[];
  /** Fired with the fields a row wants written (AGT-037's one handler). */
  onChange: (patch: AutomationPatch) => void;
}

export function AutomationRows({ project, states, onChange }: AutomationRowsProps): React.ReactElement {
  return (
    <>
      <ArchiveRow project={project} onChange={onChange} />
      <CloseRow project={project} states={states} onChange={onChange} />
    </>
  );
}

function DetailShell({ children }: { children: React.ReactNode }): React.ReactElement {
  return (
    <div className="flex w-full items-center justify-between gap-(--space-3) rounded-(--radius-control) border border-(--border) bg-(--surface) px-(--space-4) py-(--space-3)">
      {children}
    </div>
  );
}

function Lead({ children }: { children: React.ReactNode }): React.ReactElement {
  return <span className="text-meta w-1/2 font-medium text-(--text)">{children}</span>;
}

interface RowProps {
  project: ProjectDetails | undefined;
  onChange: (patch: AutomationPatch) => void;
}

function ArchiveRow({ project, onChange }: RowProps): React.ReactElement {
  const [dialogOpen, setDialogOpen] = React.useState(false);
  const delay = project?.archive_in ?? 0;
  const open = isDelayEnabled(delay);

  return (
    <>
      <CustomMonthDialog
        open={dialogOpen}
        onOpenChange={setDialogOpen}
        onSubmit={(months) => onChange({ archive_in: months })}
      />
      <AutomationRow
        title={ARCHIVE_ROW_TITLE}
        description={ARCHIVE_ROW_DESCRIPTION}
        icon={<PackageCheck size={16} strokeWidth={1.5} />}
        checked={open}
        onToggle={(next) => onChange(archiveTogglePatch(next))}
      >
        <DetailShell>
          <Lead>{ARCHIVE_DELAY_LEAD}</Lead>
          <div className="w-1/2">
            <DelayPicker
              value={delay}
              label={ARCHIVE_DELAY_LEAD}
              onSelect={(months) => onChange({ archive_in: months })}
              onCustomize={() => setDialogOpen(true)}
            />
          </div>
        </DetailShell>
      </AutomationRow>
    </>
  );
}

function CloseRow({ project, states, onChange }: RowProps & { states: readonly State[] }): React.ReactElement {
  const [dialogOpen, setDialogOpen] = React.useState(false);
  const delay = project?.close_in ?? 0;
  const open = isDelayEnabled(delay);
  const cancelled = React.useMemo(() => selectCancelledStates(states), [states]);
  const target = selectCloseTarget(project, cancelled);

  return (
    <>
      <CustomMonthDialog
        open={dialogOpen}
        onOpenChange={setDialogOpen}
        onSubmit={(months) => onChange({ close_in: months })}
      />
      <AutomationRow
        title={CLOSE_ROW_TITLE}
        description={CLOSE_ROW_DESCRIPTION}
        icon={<PackageMinus size={16} strokeWidth={1.5} />}
        checked={open}
        onToggle={(next) => onChange(closeTogglePatch(next, cancelled))}
      >
        <div className="flex flex-col rounded-(--radius-control) border border-(--border) bg-(--surface)">
          <div className="px-(--space-4) pt-(--space-3)">
            <DetailShell>
              <Lead>{CLOSE_DELAY_LEAD}</Lead>
              <div className="w-1/2">
                <DelayPicker
                  value={delay}
                  label={CLOSE_DELAY_LEAD}
                  onSelect={(months) => onChange({ close_in: months })}
                  onCustomize={() => setDialogOpen(true)}
                />
              </div>
            </DetailShell>
          </div>
          <div className="px-(--space-4) py-(--space-3)">
            <DetailShell>
              <Lead>{CLOSE_STATE_LEAD}</Lead>
              <div className="w-1/2">
                <CloseTargetPicker
                  states={cancelled}
                  selected={target}
                  onSelect={(stateId) => onChange({ default_state: stateId })}
                />
              </div>
            </DetailShell>
          </div>
        </div>
      </AutomationRow>
    </>
  );
}
