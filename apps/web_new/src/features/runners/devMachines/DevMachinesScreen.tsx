// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Dev-machines screen (NEWFRONT-555, RUN-037–041; built-in engine design
// §4.10, §11). Owns the table, the row action confirmations, the machine
// settings (rename, engine pause, teammates' runs, engine sign-out, the
// "Replaced by" merge) and the install section. The add-runner control is
// a slot: its own slice mounts the shared modal here without the page
// having to know how that modal is built.

import { isApiError, type DevMachine, type DevMachinePatch, type Runner } from "@pidash/api-client";
import * as React from "react";

import { notifyError, useDocumentHead } from "../../../shared/shell/index.js";

import { DevMachineActionDialog } from "./DevMachineActionDialog.js";
import {
  DevMachineConfirmDialog,
  DevMachineRenameDialog,
  SIGN_OUT_ENGINE_COPY,
  REMOVE_ENGINE_COPY,
  replaceMachineCopy,
} from "./DevMachineSettingsDialogs.js";
import { DevMachinesTable, type MachineSettingRequest } from "./DevMachinesTable.js";
import { devMachineName, type MachineAction } from "./model.js";
import { InstallSection } from "./InstallSection.js";

export const DEV_MACHINES_PAGE_TITLE = "AI Dev Machines";
export const MACHINE_ACTION_FAILED_TITLE = "Error!";
export const MACHINE_ACTION_FAILED_BODY: Record<MachineAction, string> = {
  rotate: "Could not rotate the dev machine token.",
  revoke: "Could not revoke the dev machine.",
  delete: "Could not remove the computer.",
};
export const MACHINE_SETTING_FAILED_BODY = {
  rename: "Could not rename the computer.",
  renameUnsupported: "This server cannot rename computers yet. Update Pi Dash on the server and try again.",
  pause: "Could not change the built-in engine's pause.",
  teamRuns: "Could not change who can start runs on this computer's engine.",
  signOutEngine: "Could not sign out the built-in engine.",
  merge: "Could not merge the computers.",
  removeEngine: "Could not remove the built-in engine from the project.",
  forbidden: "Only the computer's owner can change this.",
} as const;

export function devMachinesDocumentTitle(workspaceName: string | undefined): string {
  return workspaceName === undefined ? DEV_MACHINES_PAGE_TITLE : `${workspaceName} - ${DEV_MACHINES_PAGE_TITLE}`;
}

export interface DevMachinesScreenProps {
  workspaceSlug: string;
  workspaceName?: string | undefined;
  workspaceId: string | undefined;
  /** Extra controls rendered beside the heading (the add-runner entry). */
  actions?: React.ReactNode;
  /** Reads backing the table; omitted in tests that render the table alone. */
  machines: DevMachine[] | undefined;
  loading: boolean;
  failed: boolean;
  /** Workspace runners, engines included, for the expanded rows. */
  runners?: Runner[] | undefined;
  onRotate: (machineId: string) => Promise<unknown>;
  onRevoke: (machineId: string) => Promise<unknown>;
  onDelete: (machineId: string) => Promise<unknown>;
  /** Rename or engine control change; settings stay inert without it. */
  onUpdate?: ((machineId: string, patch: DevMachinePatch) => Promise<unknown>) | undefined;
  onSignOutEngine?: ((machineId: string) => Promise<unknown>) | undefined;
  onMerge?: ((sourceId: string, targetId: string) => Promise<unknown>) | undefined;
  /**
   * "Remove from project" for one engine runner (design §11): revokes the
   * row and keeps the engine's folder on the computer.
   */
  onRemoveEngine?: ((runnerId: string) => Promise<unknown>) | undefined;
}

type Pending = { action: MachineAction; machine: DevMachine };

type Editing =
  | { kind: "rename"; machine: DevMachine }
  | { kind: "sign-out-engine"; machine: DevMachine }
  | { kind: "replace"; machine: DevMachine; target: DevMachine }
  | { kind: "remove-engine"; machine: DevMachine; runner: Runner };

/** The toast body for a failed settings write. */
export function settingFailureBody(error: unknown, fallback: string, rename = false): string {
  if (isApiError(error)) {
    if (error.status === 403) return MACHINE_SETTING_FAILED_BODY.forbidden;
    // Servers before the rename change answer 400 to a label in the body.
    if (rename && error.status === 400) return MACHINE_SETTING_FAILED_BODY.renameUnsupported;
  }
  return fallback;
}

export function DevMachinesScreen({
  workspaceSlug,
  workspaceName,
  workspaceId,
  actions,
  machines,
  loading,
  failed,
  runners,
  onRotate,
  onRevoke,
  onDelete,
  onUpdate,
  onSignOutEngine,
  onMerge,
  onRemoveEngine,
}: DevMachinesScreenProps): React.ReactElement {
  useDocumentHead({ title: devMachinesDocumentTitle(workspaceName), robots: "noindex, nofollow" });
  const [pending, setPending] = React.useState<Pending | null>(null);
  const [working, setWorking] = React.useState(false);
  const [editing, setEditing] = React.useState<Editing | null>(null);
  const [busyIds, setBusyIds] = React.useState<ReadonlySet<string>>(() => new Set());
  // The workspace slug is part of the screen's identity even when the reads
  // are injected; keep it referenced so the header stays scope-aware.
  const headingId = `dev-machines-heading-${workspaceSlug}-${workspaceId ?? "none"}`;

  const run: Record<MachineAction, (id: string) => Promise<unknown>> = {
    rotate: onRotate,
    revoke: onRevoke,
    delete: onDelete,
  };

  async function confirm(current: Pending): Promise<void> {
    setWorking(true);
    try {
      await run[current.action](current.machine.id);
      setPending(null);
    } catch {
      notifyError(MACHINE_ACTION_FAILED_TITLE, MACHINE_ACTION_FAILED_BODY[current.action]);
    } finally {
      setWorking(false);
    }
  }

  function markBusy(id: string, busy: boolean): void {
    setBusyIds((current) => {
      const next = new Set(current);
      if (busy) next.add(id);
      else next.delete(id);
      return next;
    });
  }

  async function toggleSetting(machine: DevMachine, patch: DevMachinePatch, fallback: string): Promise<void> {
    if (onUpdate === undefined) return;
    markBusy(machine.id, true);
    try {
      await onUpdate(machine.id, patch);
    } catch (error) {
      notifyError(MACHINE_ACTION_FAILED_TITLE, settingFailureBody(error, fallback));
    } finally {
      markBusy(machine.id, false);
    }
  }

  async function runEdit(write: () => Promise<unknown>, failure: (error: unknown) => string): Promise<void> {
    setWorking(true);
    try {
      await write();
      setEditing(null);
    } catch (error) {
      notifyError(MACHINE_ACTION_FAILED_TITLE, failure(error));
    } finally {
      setWorking(false);
    }
  }

  function handleSetting(request: MachineSettingRequest): void {
    switch (request.kind) {
      case "pause":
        void toggleSetting(
          request.machine,
          { engine_paused_by_owner: request.paused },
          MACHINE_SETTING_FAILED_BODY.pause
        );
        return;
      case "team-runs":
        void toggleSetting(
          request.machine,
          { engine_team_runs: request.enabled },
          MACHINE_SETTING_FAILED_BODY.teamRuns
        );
        return;
      default:
        setEditing(request);
    }
  }

  const settingsAvailable =
    onUpdate !== undefined || onSignOutEngine !== undefined || onMerge !== undefined || onRemoveEngine !== undefined;

  let editDialog: React.ReactNode = null;
  if (editing?.kind === "rename" && onUpdate !== undefined) {
    const target = editing.machine;
    editDialog = (
      <DevMachineRenameDialog
        machineName={devMachineName(target)}
        initialLabel={target.label}
        hostLabel={target.host_label}
        pending={working}
        onCancel={() => setEditing(null)}
        onSave={(label) =>
          void runEdit(
            () => onUpdate(target.id, { label }),
            (error) => settingFailureBody(error, MACHINE_SETTING_FAILED_BODY.rename, true)
          )
        }
      />
    );
  } else if (editing?.kind === "sign-out-engine" && onSignOutEngine !== undefined) {
    const target = editing.machine;
    editDialog = (
      <DevMachineConfirmDialog
        copy={SIGN_OUT_ENGINE_COPY}
        machineName={devMachineName(target)}
        pending={working}
        onCancel={() => setEditing(null)}
        onConfirm={() =>
          void runEdit(
            () => onSignOutEngine(target.id),
            (error) => settingFailureBody(error, MACHINE_SETTING_FAILED_BODY.signOutEngine)
          )
        }
      />
    );
  } else if (editing?.kind === "remove-engine" && onRemoveEngine !== undefined) {
    const { machine: host, runner } = editing;
    editDialog = (
      <DevMachineConfirmDialog
        copy={REMOVE_ENGINE_COPY}
        machineName={`${devMachineName(host)} · ${runner.pod_detail.project_identifier}`}
        pending={working}
        onCancel={() => setEditing(null)}
        onConfirm={() =>
          void runEdit(
            () => onRemoveEngine(runner.id),
            (error) => settingFailureBody(error, MACHINE_SETTING_FAILED_BODY.removeEngine)
          )
        }
      />
    );
  } else if (editing?.kind === "replace" && onMerge !== undefined) {
    const { machine: source, target } = editing;
    editDialog = (
      <DevMachineConfirmDialog
        copy={replaceMachineCopy(devMachineName(source), devMachineName(target))}
        machineName={`${devMachineName(source)} → ${devMachineName(target)}`}
        pending={working}
        onCancel={() => setEditing(null)}
        onConfirm={() =>
          void runEdit(
            () => onMerge(source.id, target.id),
            (error) => settingFailureBody(error, MACHINE_SETTING_FAILED_BODY.merge)
          )
        }
      />
    );
  }

  return (
    <section aria-labelledby={headingId} className="flex flex-col gap-(--space-6) p-(--space-6)">
      <div className="flex flex-wrap items-center justify-between gap-(--space-3)">
        <h1 id={headingId} className="text-h2 text-(--text)">
          {DEV_MACHINES_PAGE_TITLE}
        </h1>
        {actions}
      </div>
      <DevMachinesTable
        machines={machines}
        loading={loading}
        failed={failed}
        runners={runners}
        busyMachineIds={busyIds}
        onAction={(action, machine) => setPending({ action, machine })}
        onSetting={settingsAvailable ? handleSetting : undefined}
      />
      <InstallSection />
      {pending === null ? null : (
        <DevMachineActionDialog
          open
          action={pending.action}
          machineName={devMachineName(pending.machine)}
          pending={working}
          onCancel={() => setPending(null)}
          onConfirm={() => void confirm(pending)}
        />
      )}
      {editDialog}
    </section>
  );
}
