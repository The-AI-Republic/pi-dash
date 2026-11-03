// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Dev-machines screen (NEWFRONT-555, RUN-037–041). Owns the table, the three
// action confirmations and the install section. The add-runner control is a
// slot: its own slice mounts the shared modal here without the page having to
// know how that modal is built.

import type { DevMachine } from "@pidash/api-client";
import * as React from "react";

import { notifyError, useDocumentHead } from "../../../shared/shell/index.js";

import { DevMachineActionDialog } from "./DevMachineActionDialog.js";
import { DevMachinesTable } from "./DevMachinesTable.js";
import { devMachineName, type MachineAction } from "./model.js";
import { InstallSection } from "./InstallSection.js";

export const DEV_MACHINES_PAGE_TITLE = "AI Dev Machines";
export const MACHINE_ACTION_FAILED_TITLE = "Error!";
export const MACHINE_ACTION_FAILED_BODY: Record<MachineAction, string> = {
  rotate: "Could not rotate the dev machine token.",
  revoke: "Could not revoke the dev machine.",
  delete: "Could not delete the dev machine.",
};

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
  onRotate: (machineId: string) => Promise<unknown>;
  onRevoke: (machineId: string) => Promise<unknown>;
  onDelete: (machineId: string) => Promise<unknown>;
}

type Pending = { action: MachineAction; machine: DevMachine };

export function DevMachinesScreen({
  workspaceSlug,
  workspaceName,
  workspaceId,
  actions,
  machines,
  loading,
  failed,
  onRotate,
  onRevoke,
  onDelete,
}: DevMachinesScreenProps): React.ReactElement {
  useDocumentHead({ title: devMachinesDocumentTitle(workspaceName), robots: "noindex, nofollow" });
  const [pending, setPending] = React.useState<Pending | null>(null);
  const [working, setWorking] = React.useState(false);
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
        onAction={(action, machine) => setPending({ action, machine })}
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
    </section>
  );
}
