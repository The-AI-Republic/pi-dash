// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Uninstall confirmation (parity/specs/agents.md §3, AGT-012). Removing an
// install is destructive only on this project: firing stops, the
// workspace definition stays in the catalog and can be installed again.
// A failed removal keeps the dialog and its explanation in place.

import type { SchedulerBinding } from "@pidash/api-client";
import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

import { useUninstallSchedulerBinding } from "./installData.js";

export interface UninstallInstallDialogProps {
  workspaceSlug: string;
  projectId: string;
  /** The install being removed; null keeps the dialog closed. */
  binding: SchedulerBinding | null;
  onClose: () => void;
  /** Runs after a successful removal; the detail page navigates back here. */
  onRemoved?: (() => void) | undefined;
}

/** The consequence sentence naming the definition; shared with tests. */
export function uninstallConsequence(binding: Pick<SchedulerBinding, "scheduler_name">): string {
  return `Uninstalling ${binding.scheduler_name} stops firing on this project. The scheduler definition itself is unaffected: it stays in the workspace catalog and can be installed again later.`;
}

interface UninstallActionsProps {
  pending: boolean;
  onClose: () => void;
  onConfirm: () => void;
}

function UninstallActions({ pending, onClose, onConfirm }: UninstallActionsProps): React.ReactElement {
  return (
    <div className="flex justify-end gap-(--space-3)">
      <Button variant="secondary" disabled={pending} onClick={onClose}>
        Cancel
      </Button>
      <Button variant="danger" loading={pending} disabled={pending} onClick={onConfirm}>
        {pending ? "Uninstalling…" : "Uninstall"}
      </Button>
    </div>
  );
}

export function UninstallInstallDialog({
  workspaceSlug,
  projectId,
  binding,
  onClose,
  onRemoved,
}: UninstallInstallDialogProps): React.ReactElement {
  const remove = useUninstallSchedulerBinding(workspaceSlug);
  const pending = remove.isPending;

  async function confirm(target: SchedulerBinding): Promise<void> {
    try {
      await remove.mutateAsync({ projectId, bindingId: target.id });
      onRemoved?.();
      onClose();
    } catch {
      // The notice is already raised; keeping the dialog lets the person
      // retry once the cause (a stale list, a lost seat) is understood.
    }
  }

  return (
    <Dialog
      open={binding !== null}
      onOpenChange={(next) => {
        if (!next && !pending) onClose();
      }}
      nonDismissable={pending}
      title="Uninstall scheduler?"
      description={binding === null ? "" : uninstallConsequence(binding)}
      actions={
        binding === null ? null : (
          <UninstallActions pending={pending} onClose={onClose} onConfirm={() => void confirm(binding)} />
        )
      }
    >
      {null}
    </Dialog>
  );
}
