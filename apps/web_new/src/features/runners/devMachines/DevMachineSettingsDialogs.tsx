// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Machine settings dialogs (built-in engine design §4.10, §11): the rename
// form, and the confirmation shared by "Sign out the engine" and
// "Replaced by". While a write is in flight every dismissal path is inert;
// a failure keeps the dialog open for a retry while the page toasts.

import { Button, Dialog, Input } from "@pidash/kit";
import * as React from "react";

import { REMOVE_ENGINE_BODY, REMOVE_ENGINE_LABEL, REMOVE_ENGINE_TITLE } from "../components/RunnerConfirmDialog.js";

export const RENAME_MACHINE_TITLE = "Rename computer";
export const RENAME_MACHINE_DESCRIPTION =
  "The name shows everywhere this computer appears, including its built-in engine (“Built-in engine · <name>”).";
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
export const REMOVE_ENGINE_COPY: MachineConfirmCopy = {
  title: REMOVE_ENGINE_TITLE,
  body: REMOVE_ENGINE_BODY,
  confirm: REMOVE_ENGINE_LABEL,
};

export const SIGN_OUT_ENGINE_COPY: MachineConfirmCopy = {
  title: "Sign out the built-in engine?",
  body: "This revokes the engine's credential on this computer only. Its engine stops taking runs until someone signs it in again on the computer (`pidash login --engine` or the desktop app). Pi Dash and the computer's own-agent runners stay signed in.",
  confirm: "Sign out the engine",
};

/** Confirmation copy for merging a replaced computer into its successor. */
export function replaceMachineCopy(sourceName: string, targetName: string): MachineConfirmCopy {
  return {
    title: `Is ${targetName} the computer that replaced ${sourceName}?`,
    body: `The runners of ${sourceName} move to ${targetName} with their pods, assigned issues and queued runs, and ${sourceName} is retired. Built-in engine runners are set up again on ${targetName}; own-agent runners need setting up again there. Only confirm if both are the same computer.`,
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
