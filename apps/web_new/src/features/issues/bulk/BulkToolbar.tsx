// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud bulk toolbar (parity/specs/issues.md §10, ISS-112–115). One
// request applies a property set across the selection; delete asks for
// confirmation first; archive and subscribe act immediately. The update
// form starts with every property unarmed, so only the fields a reader
// turns on travel — an untouched array is not accidentally unioned into
// every issue. Cycle, module and estimate fields follow the project's
// feature flags. The toolbar itself never mounts on the community build;
// the edition seam decides that before rendering.

import type { BulkIssueProperties } from "@pidash/api-client";
import { Button, Dialog } from "@pidash/kit";
import { Archive, Bell, CalendarDays, Repeat, Tag, Trash2, UserRound } from "lucide-react";
import * as React from "react";

import { DatePicker } from "../dropdowns/DatePicker.js";
import { LabelPicker } from "../labels/LabelPicker.js";
import {
  CyclePicker,
  EstimatePicker,
  MemberPicker,
  ModulePicker,
  PriorityPicker,
  StatePicker,
} from "../../../shared/pickers/index.js";
import type { PriorityLevel } from "../../../shared/pickers/index.js";
import { selectionStateGate, selectionStateRefusal } from "../edition/workflow.js";
import { visibleBulkFields, type BulkPropertyKey } from "./model.js";

export interface BulkToolbarProps {
  workspaceSlug: string;
  projectId: string;
  count: number;
  /** The selected rows' current states, so the state picker can gate per row. */
  fromStateIds?: readonly (string | null)[] | undefined;
  hideCycle: boolean;
  hideModule: boolean;
  hideEstimate: boolean;
  weekStart?: number | undefined;
  busy: boolean;
  onClear: () => void;
  onUpdate: (properties: BulkIssueProperties) => void;
  onDelete: () => void;
  onArchive: () => void;
  onSubscribe: () => void;
}

interface ArmedProperties {
  state_id: boolean;
  priority: boolean;
  assignee_ids: boolean;
  label_ids: boolean;
  start_date: boolean;
  target_date: boolean;
  cycle_id: boolean;
  module_ids: boolean;
  estimate_point: boolean;
}

const UNARMED: ArmedProperties = {
  state_id: false,
  priority: false,
  assignee_ids: false,
  label_ids: false,
  start_date: false,
  target_date: false,
  cycle_id: false,
  module_ids: false,
  estimate_point: false,
};

const FIELD_LABELS: Record<BulkPropertyKey, string> = {
  state_id: "State",
  priority: "Priority",
  assignee_ids: "Assignees",
  label_ids: "Labels",
  start_date: "Start date",
  target_date: "Due date",
  cycle_id: "Cycle",
  module_ids: "Modules",
  estimate_point: "Estimate",
};

const FIELD_ICONS: Record<BulkPropertyKey, React.ReactNode> = {
  state_id: <span className="size-4 rounded-full border border-(--border)" aria-hidden="true" />,
  priority: <span className="size-4 rounded-(--radius-control) bg-(--warning)" aria-hidden="true" />,
  assignee_ids: <UserRound size={16} strokeWidth={1.5} aria-hidden="true" />,
  label_ids: <Tag size={16} strokeWidth={1.5} aria-hidden="true" />,
  start_date: <CalendarDays size={16} strokeWidth={1.5} aria-hidden="true" />,
  target_date: <CalendarDays size={16} strokeWidth={1.5} aria-hidden="true" />,
  cycle_id: <Repeat size={16} strokeWidth={1.5} aria-hidden="true" />,
  module_ids: <span className="size-4 rounded-(--radius-control) border border-(--border)" aria-hidden="true" />,
  estimate_point: (
    <span className="text-caption font-medium" aria-hidden="true">
      #
    </span>
  ),
};

/** Build the request payload from only the armed fields. */
export function armedBulkProperties(
  armed: ArmedProperties,
  values: {
    stateId: string | null;
    priority: PriorityLevel;
    assigneeIds: readonly string[];
    labelIds: readonly string[];
    startDate: string | null;
    targetDate: string | null;
    cycleId: string | null;
    moduleIds: readonly string[];
    estimatePoint: string | null;
  }
): BulkIssueProperties {
  const properties: BulkIssueProperties = {};
  if (armed.state_id && values.stateId !== null) properties.state_id = values.stateId;
  if (armed.priority) properties.priority = values.priority;
  if (armed.assignee_ids) properties.assignee_ids = [...values.assigneeIds];
  if (armed.label_ids) properties.label_ids = [...values.labelIds];
  if (armed.start_date) properties.start_date = values.startDate;
  if (armed.target_date) properties.target_date = values.targetDate;
  if (armed.cycle_id) properties.cycle_id = values.cycleId;
  if (armed.module_ids) properties.module_ids = [...values.moduleIds];
  if (armed.estimate_point) properties.estimate_point = values.estimatePoint;
  return properties;
}

export function BulkToolbar(props: BulkToolbarProps): React.ReactElement | null {
  const [updateOpen, setUpdateOpen] = React.useState(false);
  const [deleteOpen, setDeleteOpen] = React.useState(false);
  const [stateError, setStateError] = React.useState<string | null>(null);
  const [armed, setArmed] = React.useState<ArmedProperties>(UNARMED);
  const [stateId, setStateId] = React.useState<string | null>(null);
  const [priority, setPriority] = React.useState<PriorityLevel>("none");
  const [assigneeIds, setAssigneeIds] = React.useState<string[]>([]);
  const [labelIds, setLabelIds] = React.useState<string[]>([]);
  const [startDate, setStartDate] = React.useState<string | null>(null);
  const [targetDate, setTargetDate] = React.useState<string | null>(null);
  const [cycleId, setCycleId] = React.useState<string | null>(null);
  const [moduleIds, setModuleIds] = React.useState<string[]>([]);
  const [estimatePoint, setEstimatePoint] = React.useState<string | null>(null);

  const fields = visibleBulkFields(props);
  const fromStateIds = props.fromStateIds ?? [];
  // One target has to be open to every selected row (ISS-233); without
  // the workflow this is undefined and the picker stays plain.
  const stateGate = selectionStateGate(props.projectId, fromStateIds);
  const selectionLabel = props.count === 1 ? "1 selected" : `${props.count} selected`;

  function toggle(key: BulkPropertyKey): void {
    // Un-arming drops the state neither the rules nor the batch knows any
    // more; re-arming starts from a target the reader picks again.
    if (key === "state_id") setStateError(null);
    setArmed((current) => ({ ...current, [key]: !current[key] }));
  }

  function submitUpdate(): void {
    // The picker greys the rows it refuses, but a target picked before
    // the selection changed could still be held here. Ask the rules once
    // more against the rows that are selected now, so the batch is never
    // posted for a row the workflow refuses (ISS-233).
    if (armed.state_id && stateId !== null) {
      const refusal = selectionStateRefusal(props.projectId, fromStateIds, stateId);
      if (refusal !== null) {
        setStateError(refusal.reason);
        return;
      }
    }
    const properties = armedBulkProperties(armed, {
      stateId,
      priority,
      assigneeIds,
      labelIds,
      startDate,
      targetDate,
      cycleId,
      moduleIds,
      estimatePoint,
    });
    if (Object.keys(properties).length === 0) return;
    props.onUpdate(properties);
    setUpdateOpen(false);
  }

  return (
    <>
      <div
        data-testid="bulk-toolbar"
        className="sticky bottom-0 z-20 flex flex-wrap items-center gap-(--space-2) border-t border-(--border) bg-(--surface) px-(--space-4) py-(--space-3)"
      >
        <span className="text-emphasis text-(--text)">{selectionLabel}</span>
        <Button variant="secondary" size="small" onClick={() => setUpdateOpen(true)} disabled={props.busy}>
          Edit properties
        </Button>
        <Button variant="secondary" size="small" onClick={props.onSubscribe} disabled={props.busy}>
          <Bell size={14} strokeWidth={1.5} aria-hidden="true" />
          Subscribe
        </Button>
        <Button variant="secondary" size="small" onClick={props.onArchive} disabled={props.busy}>
          <Archive size={14} strokeWidth={1.5} aria-hidden="true" />
          Archive
        </Button>
        <Button variant="danger" size="small" onClick={() => setDeleteOpen(true)} disabled={props.busy}>
          <Trash2 size={14} strokeWidth={1.5} aria-hidden="true" />
          Delete
        </Button>
        <button
          type="button"
          onClick={props.onClear}
          className="text-caption ml-auto text-(--text-muted) hover:text-(--text)"
        >
          Clear selection
        </button>
      </div>

      <Dialog
        open={updateOpen}
        onOpenChange={setUpdateOpen}
        title="Edit selected work items"
        description="Turn on a property to apply it to every selected work item."
        actions={
          <>
            <Button variant="secondary" onClick={() => setUpdateOpen(false)}>
              Cancel
            </Button>
            <Button variant="primary" onClick={submitUpdate} disabled={Object.values(armed).every((value) => !value)}>
              Apply
            </Button>
          </>
        }
      >
        <div className="flex flex-col gap-(--space-3)">
          {fields.map((key) => (
            <div key={key} className="grid grid-cols-[minmax(9rem,auto)_1fr] items-center gap-(--space-3)">
              <label className="text-body flex items-center gap-(--space-2) text-(--text)">
                <input
                  type="checkbox"
                  aria-label={FIELD_LABELS[key]}
                  checked={armed[key]}
                  onChange={() => toggle(key)}
                  className="size-4 accent-(--accent)"
                />
                {FIELD_ICONS[key]}
                {FIELD_LABELS[key]}
              </label>
              <div
                className={armed[key] ? "" : "pointer-events-none opacity-40"}
                aria-hidden={armed[key] ? undefined : true}
              >
                {key === "state_id" ? (
                  <>
                    <StatePicker
                      workspaceSlug={props.workspaceSlug}
                      projectId={props.projectId}
                      value={stateId}
                      onChange={(nextStateId) => {
                        setStateError(null);
                        setStateId(nextStateId);
                      }}
                      disabled={!armed.state_id}
                      optionGate={stateGate}
                    />
                    {stateError === null ? null : (
                      <p role="alert" className="text-meta text-(--danger)">
                        {stateError}
                      </p>
                    )}
                  </>
                ) : null}
                {key === "priority" ? (
                  <PriorityPicker value={priority} onChange={setPriority} disabled={!armed.priority} />
                ) : null}
                {key === "assignee_ids" ? (
                  <MemberPicker
                    workspaceSlug={props.workspaceSlug}
                    projectId={props.projectId}
                    multiple
                    value={assigneeIds}
                    onChange={setAssigneeIds}
                    disabled={!armed.assignee_ids}
                  />
                ) : null}
                {key === "label_ids" ? (
                  <LabelPicker
                    workspaceSlug={props.workspaceSlug}
                    projectId={props.projectId}
                    value={labelIds}
                    onChange={setLabelIds}
                    disabled={!armed.label_ids}
                  />
                ) : null}
                {key === "start_date" ? (
                  <DatePicker
                    label="Start date"
                    value={startDate}
                    onChange={setStartDate}
                    max={targetDate}
                    weekStart={props.weekStart}
                    disabled={!armed.start_date}
                  />
                ) : null}
                {key === "target_date" ? (
                  <DatePicker
                    label="Due date"
                    value={targetDate}
                    onChange={setTargetDate}
                    min={startDate}
                    weekStart={props.weekStart}
                    disabled={!armed.target_date}
                  />
                ) : null}
                {key === "cycle_id" ? (
                  <CyclePicker
                    workspaceSlug={props.workspaceSlug}
                    projectId={props.projectId}
                    value={cycleId}
                    onChange={setCycleId}
                    disabled={!armed.cycle_id}
                  />
                ) : null}
                {key === "module_ids" ? (
                  <ModulePicker
                    workspaceSlug={props.workspaceSlug}
                    projectId={props.projectId}
                    multiple
                    value={moduleIds}
                    onChange={setModuleIds}
                    disabled={!armed.module_ids}
                  />
                ) : null}
                {key === "estimate_point" ? (
                  <EstimatePicker
                    workspaceSlug={props.workspaceSlug}
                    projectId={props.projectId}
                    value={estimatePoint}
                    onChange={setEstimatePoint}
                    disabled={!armed.estimate_point}
                  />
                ) : null}
              </div>
            </div>
          ))}
        </div>
      </Dialog>

      <Dialog
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        title="Delete selected work items?"
        description="This permanently removes every selected work item. It cannot be undone."
        actions={
          <>
            <Button variant="secondary" onClick={() => setDeleteOpen(false)}>
              Cancel
            </Button>
            <Button
              variant="danger"
              onClick={() => {
                setDeleteOpen(false);
                props.onDelete();
              }}
            >
              Delete
            </Button>
          </>
        }
      >
        <p className="text-body text-(--text-muted)">
          {props.count === 1 ? "1 work item will be deleted." : `${props.count} work items will be deleted.`}
        </p>
      </Dialog>
    </>
  );
}
