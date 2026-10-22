// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Dev-machines table (NEWFRONT-555, RUN-037). One row per machine with the
// derived status, its runner counts and the row actions. Loading, failure
// and empty each occupy a full-width row inside the same table, so the
// page chrome never collapses while the list is unsettled.

import { Badge, Button } from "@pidash/kit";
import type { DevMachine } from "@pidash/api-client";
import * as React from "react";

import {
  DEV_MACHINE_STATUS_LABELS,
  DEV_MACHINE_STATUS_TONES,
  devMachineName,
  devMachineStatus,
  devMachineSubline,
  machineActions,
  machineTime,
  runnerCountLabel,
  type DevMachineStatus,
  type MachineAction,
} from "./model.js";

export const DEV_MACHINES_HEADING = "AI Dev Machines";
export const DEV_MACHINES_EMPTY = "No dev machines registered for this workspace yet.";
export const DEV_MACHINES_LOADING = "Loading dev machines…";
export const DEV_MACHINES_FAILED = "Could not load dev machines. The list retries automatically.";
export const MACHINE_ACTION_LABELS: Record<MachineAction, string> = {
  rotate: "Rotate",
  revoke: "Revoke",
  delete: "Delete",
};

export interface DevMachinesTableProps {
  machines: DevMachine[] | undefined;
  loading: boolean;
  failed: boolean;
  onAction: (action: MachineAction, machine: DevMachine) => void;
  formatTime?: ((iso: string) => string) | undefined;
}

export function DevMachineStatusBadge({ status }: { status: DevMachineStatus }): React.ReactElement {
  return (
    <button type="button" tabIndex={-1} className="contents cursor-default">
      <Badge tone={DEV_MACHINE_STATUS_TONES[status]} dot>
        {DEV_MACHINE_STATUS_LABELS[status]}
      </Badge>
    </button>
  );
}

const CELL = "px-(--space-3) py-(--space-2)";

export function DevMachinesTable({
  machines,
  loading,
  failed,
  onAction,
  formatTime,
}: DevMachinesTableProps): React.ReactElement {
  const rows = machines ?? [];
  let stateRow: React.ReactNode = null;
  if (failed) {
    stateRow = (
      <tr>
        <td colSpan={6} className="text-body px-(--space-3) py-(--space-8) text-center text-(--danger)">
          {DEV_MACHINES_FAILED}
        </td>
      </tr>
    );
  } else if (loading) {
    stateRow = (
      <tr>
        <td
          colSpan={6}
          className="text-body px-(--space-3) py-(--space-8) text-center text-(--text-muted)"
          role="status"
        >
          {DEV_MACHINES_LOADING}
        </td>
      </tr>
    );
  } else if (rows.length === 0) {
    stateRow = (
      <tr>
        <td colSpan={6} className="text-body px-(--space-3) py-(--space-8) text-center text-(--text-muted)">
          {DEV_MACHINES_EMPTY}
        </td>
      </tr>
    );
  }
  return (
    <div className="overflow-x-auto rounded-(--radius-control) border border-(--border)">
      <table data-testid="dev-machines-table" className="text-body w-full text-left">
        <thead className="bg-(--subtle) text-(--text-muted)">
          <tr>
            {["Machine", "Status", "Runners", "Last seen", "Last heartbeat", "Actions"].map((label) => (
              <th key={label} scope="col" className={`${CELL} font-medium`}>
                {label === "Actions" ? <span className="sr-only">{label}</span> : label}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((machine) => {
            const status = devMachineStatus(machine);
            return (
              <tr key={machine.id} className="border-t border-(--border)">
                <td className={CELL}>
                  <div className="font-medium text-(--text)">{devMachineName(machine)}</div>
                  <div className="text-meta font-mono text-(--text-muted)">{devMachineSubline(machine)}</div>
                </td>
                <td className={CELL}>
                  <DevMachineStatusBadge status={status} />
                </td>
                <td className={CELL}>{runnerCountLabel(machine)}</td>
                <td className={CELL}>{machineTime(machine.last_seen_at, formatTime)}</td>
                <td className={CELL}>{machineTime(machine.last_heartbeat_at, formatTime)}</td>
                <td className={`${CELL} text-right`}>
                  <div className="flex justify-end gap-(--space-2)">
                    {machineActions(machine).map((action) => (
                      <Button
                        key={action}
                        size="small"
                        variant={action === "rotate" ? "secondary" : "danger"}
                        onClick={() => onAction(action, machine)}
                      >
                        {MACHINE_ACTION_LABELS[action]}
                      </Button>
                    ))}
                  </div>
                </td>
              </tr>
            );
          })}
          {stateRow}
        </tbody>
      </table>
    </div>
  );
}
