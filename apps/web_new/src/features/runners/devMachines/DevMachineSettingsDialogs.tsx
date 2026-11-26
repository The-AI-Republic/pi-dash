// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Machine settings dialogs (Pi Dash Agent design §4.10, §11): the rename
// form, and the confirmation shared by "Replaced by" and "Remove from
// project". While a write is in flight every dismissal path is inert;
// a failure keeps the dialog open for a retry while the page toasts.

import { Button, Dialog, Input } from "@pidash/kit";
import * as React from "react";

import {
  REMOVE_PIDASH_AGENT_BODY,
  REMOVE_PIDASH_AGENT_LABEL,
  REMOVE_PIDASH_AGENT_TITLE,
} from "../components/RunnerConfirmDialog.js";

export const RENAME_MACHINE_TITLE = "Rename computer";
export const RENAME_MACHINE_DESCRIPTION =
  "The name shows everywhere this computer appears, including its Pi Dash Agent (“Pi Dash Agent · <name>”).";
export const RENAME_MACHINE_FIELD = "Computer name";
export const RENAME_MACHINE_HINT = "Leave empty to use the hostname.";
export const RENAME_MACHINE_SAVE = "Save";

export interface DevMachineRenameDialogProps {
  machineName: string;
  initialLabel: string;
  hostLabel: string;
  pending: boolean;
  onCancel: () => void;
  onSave: (label: string) => void;
}

export function DevMachineRenameDialog({
  machineName,
  initialLabel,
  hostLabel,
  pending,
  onCancel,
  onSave,
}: DevMachineRenameDialogProps): React.ReactElement {
  const [value, setValue] = React.useState(initialLabel);
  const trimmed = value.trim();
  const unchanged = trimmed === initialLabel.trim();
  return (
    <Dialog
      open
      onOpenChange={(next) => {
        if (!next && !pending) onCancel();
      }}
      title={RENAME_MACHINE_TITLE}
      description={RENAME_MACHINE_DESCRIPTION}
      nonDismissable={pending}
      bodyClassName="mt-(--space-4)"
      actions={
        <>
          <Button variant="ghost" aria-disabled={pending || undefined} onClick={() => !pending && onCancel()}>
            Cancel
          </Button>
          <Button variant="primary" loading={pending} disabled={unchanged} onClick={() => onSave(trimmed)}>
            {RENAME_MACHINE_SAVE}
          </Button>
        </>
      }
    >
      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (!unchanged && !pending) onSave(trimmed);
        }}
      >
        <Input
          label={RENAME_MACHINE_FIELD}
          {...(hostLabel === "" ? {} : { hint: `${RENAME_MACHINE_HINT} (${hostLabel})` })}
          value={value}
          placeholder={machineName}
          maxLength={255}
          autoFocus
          onChange={(event) => setValue(event.target.value)}
        />
      </form>
    </Dialog>
  );
}

export interface MachineConfirmCopy {
  title: string;
  body: string;
  confirm: string;
}

/** "Remove from project" for one engine runner (design §11). */
export const REMOVE_PIDASH_AGENT_COPY: MachineConfirmCopy = {
  title: REMOVE_PIDASH_AGENT_TITLE,
  body: REMOVE_PIDASH_AGENT_BODY,
  confirm: REMOVE_PIDASH_AGENT_LABEL,
};

/** Confirmation copy for merging a replaced computer into its successor. */
export function replaceMachineCopy(sourceName: string, targetName: string): MachineConfirmCopy {
  return {
    title: `Is ${targetName} the computer that replaced ${sourceName}?`,
    body: `The runners of ${sourceName} move to ${targetName} with their pods and queued runs, and ${sourceName} is retired. Pi Dash Agent runners are set up again on ${targetName}; own-agent runners need setting up again there. Only confirm if both are the same computer.`,
    confirm: "Merge",
  };
}

export interface DevMachineConfirmDialogProps {
  copy: MachineConfirmCopy;
  machineName: string;
  pending: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}

export function DevMachineConfirmDialog({
  copy,
  machineName,
  pending,
  onCancel,
  onConfirm,
}: DevMachineConfirmDialogProps): React.ReactElement {
  return (
    <Dialog
      open
      onOpenChange={(next) => {
        if (!next && !pending) onCancel();
      }}
      title={copy.title}
      description={copy.body}
      nonDismissable={pending}
      bodyClassName="mt-(--space-4)"
      actions={
        <>
          <Button variant="ghost" aria-disabled={pending || undefined} onClick={() => !pending && onCancel()}>
            Cancel
          </Button>
          <Button variant="danger" loading={pending} onClick={onConfirm}>
            {copy.confirm}
          </Button>
        </>
      }
    >
      <p className="text-body text-(--text)">
        <span className="font-medium">{machineName}</span>
      </p>
    </Dialog>
  );
}
