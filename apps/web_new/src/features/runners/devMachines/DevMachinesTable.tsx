// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Dev-machines table (NEWFRONT-555, RUN-037; built-in engine design §4.11,
// §11). One row per computer with the derived status, the built-in
// engine's state, its runner counts and the row actions. Expanding a row
// lists the computer's built-in engine (one entry per project) and its
// own-agent runners, with the computer's engine settings. Loading,
// failure and empty each occupy a full-width row inside the same table,
// so the page chrome never collapses while the list is unsettled.

import { Badge, Button, IconButton, Switch } from "@pidash/kit";
import type { DevMachine, Runner } from "@pidash/api-client";
import { ChevronDown, ChevronRight } from "lucide-react";
import * as React from "react";

import { BUILT_IN_ENGINE } from "../../../shared/lib/runnerIdentity.js";
import { REMOVE_ENGINE_LABEL } from "../components/RunnerConfirmDialog.js";
import { RunnerStatusBadge, runnerStatusOf } from "../components/RunnerStatusBadge.js";
import {
  DEV_MACHINE_STATUS_LABELS,
  DEV_MACHINE_STATUS_TONES,
  devMachineName,
  devMachineStatus,
  devMachineSubline,
  engineState,
  machineActions,
  machineSettings,
  machineTime,
  mergedIntoText,
  replacementCandidate,
  runnerCountLabel,
  runnersByMachine,
  type DevMachineStatus,
  type MachineAction,
  type MachineRunners,
} from "./model.js";

export const DEV_MACHINES_HEADING = "AI Dev Machines";
export const DEV_MACHINES_EMPTY = "No dev machines registered for this workspace yet.";
export const DEV_MACHINES_LOADING = "Loading dev machines…";
export const DEV_MACHINES_FAILED = "Could not load dev machines. The list retries automatically.";
export const MACHINE_ACTION_LABELS: Record<MachineAction, string> = {
  rotate: "Rotate",
  revoke: "Revoke",
  delete: "Remove this computer",
};

export const MACHINE_DETAILS_SHOW = "Show runners and settings";
export const MACHINE_DETAILS_HIDE = "Hide runners and settings";
export const MACHINE_ENGINE_HEADING = BUILT_IN_ENGINE;
export const MACHINE_OWN_AGENTS_HEADING = "Own-agent runners";
export const MACHINE_ENGINE_NONE = "The built-in engine is not turned on for any project on this computer.";
export const MACHINE_OWN_AGENTS_NONE = "No own-agent runners on this computer.";
export const MACHINE_RUNNERS_LOADING = "Loading runners…";
export const MACHINE_RENAME_LABEL = "Rename";
export const MACHINE_PAUSE_LABEL = "Pause the built-in engine";
export const MACHINE_RESUME_LABEL = "Resume the built-in engine";
export const MACHINE_TEAM_RUNS_LABEL = "Teammates can start runs on my issues";
export const MACHINE_TEAM_RUNS_HINT =
  "Off: only your own runs, and automatic runs on your issues, use this computer's engine and your wallet.";
export const MACHINE_SIGN_OUT_ENGINE_LABEL = "Sign out the engine";

/** "Replaced by <machine>": the merge offer for a wiped computer. */
export function replacedByLabel(targetName: string): string {
  return `Replaced by ${targetName}`;
}

/** A settings change requested from an expanded row. */
export type MachineSettingRequest =
  | { kind: "rename"; machine: DevMachine }
  | { kind: "pause"; machine: DevMachine; paused: boolean }
  | { kind: "team-runs"; machine: DevMachine; enabled: boolean }
  | { kind: "sign-out-engine"; machine: DevMachine }
  | { kind: "replace"; machine: DevMachine; target: DevMachine }
  | { kind: "remove-engine"; machine: DevMachine; runner: Runner };

/** Accessible name of one engine's "Remove from project" button on the machine row. */
export function removeEngineLabel(projectIdentifier: string): string {
  return `Remove the built-in engine from ${projectIdentifier}`;
}

export interface DevMachinesTableProps {
  machines: DevMachine[] | undefined;
  loading: boolean;
  failed: boolean;
  onAction: (action: MachineAction, machine: DevMachine) => void;
  /** Settings requests from expanded rows; without it the settings stay inert. */
  onSetting?: ((request: MachineSettingRequest) => void) | undefined;
  /** Workspace runners, engines included, for the expanded rows. */
  runners?: Runner[] | undefined;
  /** Machine ids with a settings write in flight. */
  busyMachineIds?: ReadonlySet<string> | undefined;
  formatTime?: ((iso: string) => string) | undefined;
  formatDate?: ((iso: string) => string) | undefined;
}

export function DevMachineStatusBadge({ status }: { status: DevMachineStatus }): React.ReactElement {
  return (
    <span data-testid="dev-machine-status-badge" className="inline-flex">
      <Badge tone={DEV_MACHINE_STATUS_TONES[status]} dot>
        {DEV_MACHINE_STATUS_LABELS[status]}
      </Badge>
    </span>
  );
}

const CELL = "px-(--space-3) py-(--space-2)";

function runnerProject(runner: Runner): string {
  return runner.pod_detail.project_identifier;
}

function RunnerLine({
  runner,
  label,
  onRemove,
  disabled,
}: {
  runner: Runner;
  label: string;
  onRemove?: (() => void) | undefined;
  disabled?: boolean | undefined;
}): React.ReactElement {
  return (
    <li className="flex items-center justify-between gap-(--space-3)">
      <span className="min-w-0 truncate text-(--text)">{label}</span>
      <span className="inline-flex items-center gap-(--space-2)">
        <RunnerStatusBadge status={runnerStatusOf(runner)} />
        {onRemove === undefined ? null : (
          <Button
            size="small"
            variant="secondary"
            aria-label={removeEngineLabel(runnerProject(runner))}
            disabled={disabled}
            onClick={onRemove}
          >
            {REMOVE_ENGINE_LABEL}
          </Button>
        )}
      </span>
    </li>
  );
}

function RunnerGroup({
  heading,
  empty,
  loaded,
  rows,
  label,
  onRemove,
  disabled,
}: {
  heading: string;
  empty: string;
  loaded: boolean;
  rows: Runner[];
  label: (runner: Runner) => string;
  /** Per-row remove action; absent rows show none. */
  onRemove?: ((runner: Runner) => void) | undefined;
  disabled?: boolean | undefined;
}): React.ReactElement {
  let body: React.ReactNode;
  if (!loaded) body = <p className="text-meta text-(--text-muted)">{MACHINE_RUNNERS_LOADING}</p>;
  else if (rows.length === 0) body = <p className="text-meta text-(--text-muted)">{empty}</p>;
  else
    body = (
      <ul className="flex flex-col gap-(--space-1)">
        {rows.map((runner) => (
          <RunnerLine
            key={runner.id}
            runner={runner}
            label={label(runner)}
            onRemove={onRemove === undefined ? undefined : () => onRemove(runner)}
            disabled={disabled}
          />
        ))}
      </ul>
    );
  return (
    <section aria-label={heading} className="flex flex-col gap-(--space-2)">
      <h3 className="text-meta font-medium text-(--text-muted)">{heading}</h3>
      {body}
    </section>
  );
}

interface MachineDetailsProps {
  machine: DevMachine;
  machines: DevMachine[];
  runners: MachineRunners | undefined;
  runnersLoaded: boolean;
  busy: boolean;
  onSetting: ((request: MachineSettingRequest) => void) | undefined;
}

function MachineDetails({
  machine,
  machines,
  runners,
  runnersLoaded,
  busy,
  onSetting,
}: MachineDetailsProps): React.ReactElement {
  const settings = machineSettings(machine, { hasEngine: (runners?.engines.length ?? 0) > 0 });
  const replacement = replacementCandidate(machine, machines);
  const disabled = busy || onSetting === undefined;
  const request = (next: MachineSettingRequest): void => onSetting?.(next);
  return (
    <div className="flex flex-col gap-(--space-4) bg-(--subtle) px-(--space-6) py-(--space-4)">
      <div className="grid gap-(--space-4) md:grid-cols-2">
        <RunnerGroup
          heading={MACHINE_ENGINE_HEADING}
          empty={MACHINE_ENGINE_NONE}
          loaded={runnersLoaded}
          rows={runners?.engines ?? []}
          label={runnerProject}
          onRemove={
            onSetting === undefined || machine.revoked_at !== null
              ? undefined
              : (runner) => request({ kind: "remove-engine", machine, runner })
          }
          disabled={disabled}
        />
        <RunnerGroup
          heading={MACHINE_OWN_AGENTS_HEADING}
          empty={MACHINE_OWN_AGENTS_NONE}
          loaded={runnersLoaded}
          rows={runners?.ownAgents ?? []}
          label={(runner) => `${runner.name} · ${runnerProject(runner)}`}
        />
      </div>
      {settings.teamRuns ? (
        <div className="flex flex-col gap-(--space-1)">
          <div className="flex items-center gap-(--space-2)">
            <Switch
              checked={machine.engine_team_runs === true}
              label={MACHINE_TEAM_RUNS_LABEL}
              disabled={disabled}
              onCheckedChange={(enabled) => request({ kind: "team-runs", machine, enabled })}
            />
            <span className="text-body text-(--text)">{MACHINE_TEAM_RUNS_LABEL}</span>
          </div>
          <p className="text-meta text-(--text-muted)">{MACHINE_TEAM_RUNS_HINT}</p>
        </div>
      ) : null}
      <div className="flex flex-wrap gap-(--space-2)">
        {settings.rename ? (
          <Button
            size="small"
            variant="secondary"
            disabled={disabled}
            onClick={() => request({ kind: "rename", machine })}
          >
            {MACHINE_RENAME_LABEL}
          </Button>
        ) : null}
        {settings.pause ? (
          <Button
            size="small"
            variant="secondary"
            disabled={disabled}
            onClick={() => request({ kind: "pause", machine, paused: machine.engine_paused_by_owner !== true })}
          >
            {machine.engine_paused_by_owner === true ? MACHINE_RESUME_LABEL : MACHINE_PAUSE_LABEL}
          </Button>
        ) : null}
        {settings.signOutEngine ? (
          <Button
            size="small"
            variant="secondary"
            disabled={disabled}
            onClick={() => request({ kind: "sign-out-engine", machine })}
          >
            {MACHINE_SIGN_OUT_ENGINE_LABEL}
          </Button>
        ) : null}
        {replacement === null ? null : (
          <Button
            size="small"
            variant="secondary"
            disabled={disabled}
            onClick={() => request({ kind: "replace", machine, target: replacement })}
          >
            {replacedByLabel(devMachineName(replacement))}
          </Button>
        )}
      </div>
    </div>
  );
}

export function DevMachinesTable({
  machines,
  loading,
  failed,
  onAction,
  onSetting,
  runners,
  busyMachineIds,
  formatTime,
  formatDate,
}: DevMachinesTableProps): React.ReactElement {
  const rows = machines ?? [];
  const [expanded, setExpanded] = React.useState<ReadonlySet<string>>(() => new Set());
  const grouped = React.useMemo(() => runnersByMachine(runners ?? []), [runners]);

  function toggle(id: string): void {
    setExpanded((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

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
            const engine = engineState(
              machine,
              { hasEngine: (grouped.get(machine.id)?.engines.length ?? 0) > 0 },
              formatDate
            );
            const merged = mergedIntoText(machine, rows);
            const open = expanded.has(machine.id);
            const detailsId = `dev-machine-details-${machine.id}`;
            return (
              <React.Fragment key={machine.id}>
                <tr className="border-t border-(--border)">
                  <td className={CELL}>
                    <div className="font-medium text-(--text)">{devMachineName(machine)}</div>
                    <div className="text-meta font-mono text-(--text-muted)">{devMachineSubline(machine)}</div>
                  </td>
                  <td className={CELL}>
                    <DevMachineStatusBadge status={status} />
                    {engine === null ? null : (
                      <span data-testid="dev-machine-engine-state" className="mt-(--space-1) block">
                        <Badge tone={engine.tone}>{engine.label}</Badge>
                      </span>
                    )}
                    {merged === null ? null : (
                      <span className="text-meta mt-(--space-1) block text-(--text-muted)">{merged}</span>
                    )}
                  </td>
                  <td className={CELL}>
                    <span className="inline-flex items-center gap-(--space-1)">
                      <IconButton
                        size="small"
                        label={open ? MACHINE_DETAILS_HIDE : MACHINE_DETAILS_SHOW}
                        aria-expanded={open}
                        aria-controls={open ? detailsId : undefined}
                        onClick={() => toggle(machine.id)}
                      >
                        {open ? (
                          <ChevronDown size={14} aria-hidden="true" />
                        ) : (
                          <ChevronRight size={14} aria-hidden="true" />
                        )}
                      </IconButton>
                      {runnerCountLabel(machine)}
                    </span>
                  </td>
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
                {open ? (
                  <tr id={detailsId} data-testid="dev-machine-details">
                    <td colSpan={6} className="p-0">
                      <MachineDetails
                        machine={machine}
                        machines={rows}
                        runners={grouped.get(machine.id)}
                        runnersLoaded={runners !== undefined}
                        busy={busyMachineIds?.has(machine.id) === true}
                        onSetting={onSetting}
                      />
                    </td>
                  </tr>
                ) : null}
              </React.Fragment>
            );
          })}
          {stateRow}
        </tbody>
      </table>
    </div>
  );
}
