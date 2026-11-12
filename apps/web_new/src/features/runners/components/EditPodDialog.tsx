// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pod settings dialog (NEWFRONT-542, RUN-014). The name field carries only
// the part after the project's identifier; the server re-adds the prefix on
// save. Saving computes the difference against the stored pod and sends
// that alone — an untouched form closes without a request. The default
// switch is one-way: the project's current default cannot switch itself off
// there, because the only way to move the flag is to promote another pod.

import { Button, Dialog, Input, Switch } from "@pidash/kit";
import * as React from "react";

import type { Pod } from "@pidash/api-client";
import { notifyError } from "../../../shared/shell/index.js";
import { useUpdatePod } from "../api/pods.js";
import {
  POD_ERROR_TITLE,
  POD_UPDATE_FAILED,
  podUpdatePatch,
  podWriteErrorMessage,
  seedPodEdit,
  validatePodName,
  type PodEditSeed,
} from "../pods.js";

export const EDIT_POD_TITLE = "Edit pod";
export const EDIT_POD_HINT = "Rename the pod, change its description, or make it the project's default.";
export const EDIT_POD_NAME_HINT =
  "Letters, digits, dashes and underscores. The project identifier is prefixed for you.";
export const EDIT_POD_DEFAULT_LABEL = "Project default";
export const EDIT_POD_DEFAULT_CURRENT = "This pod is the project's default. Promote a different pod to change it.";
export const EDIT_POD_DEFAULT_HINT = "New issues without a pod of their own are delegated to the project's default.";
export const EDIT_POD_SUBMIT = "Save changes";
export const EDIT_POD_SAVING = "Saving…";

export interface EditPodDialogProps {
  workspaceSlug: string;
  /** The pod to edit; null keeps the dialog closed. */
  pod: Pod | null;
  onClose: () => void;
}

function EditPodForm({
  pod,
  update,
  onClose,
}: {
  pod: Pod;
  update: ReturnType<typeof useUpdatePod>;
  onClose: () => void;
}): React.ReactElement {
  // The seed is the pod as it was when the form opened: the comparison
  // basis for the changed subset. The form mounts per pod, so a later edit
  // never starts from an earlier one's values.
  const [seed] = React.useState<PodEditSeed>(() => seedPodEdit(pod));
  const [draft, setDraft] = React.useState<PodEditSeed>(seed);
  const [nameError, setNameError] = React.useState<string | undefined>(undefined);

  function submit(event: React.FormEvent): void {
    event.preventDefault();
    const name = validatePodName(draft.name);
    if ("error" in name) {
      setNameError(name.error);
      return;
    }
    const patch = podUpdatePatch(seed, { ...draft, name: name.value });
    if (patch === null) {
      onClose();
      return;
    }
    update.mutate(
      { podId: pod.id, patch },
      {
        onSuccess: onClose,
        onError: (error) => notifyError(POD_ERROR_TITLE, podWriteErrorMessage(error, POD_UPDATE_FAILED)),
      }
    );
  }

  const currentDefault = pod.is_default;

  return (
    <form className="flex flex-col gap-(--space-5)" noValidate onSubmit={submit}>
      <Input
        label="Name"
        value={draft.name}
        placeholder="beefy"
        hint={EDIT_POD_NAME_HINT}
        {...(nameError === undefined ? {} : { error: nameError })}
        onChange={(event) => {
          const value = event.target.value;
          setDraft((current) => ({ ...current, name: value }));
          setNameError(undefined);
        }}
      />
      <Input
        label="Description (optional)"
        value={draft.description}
        placeholder="Where this pod runs, and what it is for"
        onChange={(event) => {
          const value = event.target.value;
          setDraft((current) => ({ ...current, description: value }));
        }}
      />
      <div className="flex items-start justify-between gap-(--space-4)">
        <div className="flex flex-col gap-(--space-1)">
          <span className="text-body font-medium text-(--text)">{EDIT_POD_DEFAULT_LABEL}</span>
          <span className="text-meta text-(--text-muted)">
            {currentDefault ? EDIT_POD_DEFAULT_CURRENT : EDIT_POD_DEFAULT_HINT}
          </span>
        </div>
        <Switch
          checked={draft.isDefault}
          label={EDIT_POD_DEFAULT_LABEL}
          disabled={currentDefault || update.isPending}
          onCheckedChange={(checked) => {
            setDraft((current) => ({ ...current, isDefault: checked }));
          }}
        />
      </div>
      <div className="flex justify-end gap-(--space-2)">
        <Button variant="ghost" disabled={update.isPending} onClick={onClose}>
          Cancel
        </Button>
        <Button type="submit" variant="primary" loading={update.isPending}>
          {update.isPending ? EDIT_POD_SAVING : EDIT_POD_SUBMIT}
        </Button>
      </div>
    </form>
  );
}

export function EditPodDialog({ workspaceSlug, pod, onClose }: EditPodDialogProps): React.ReactElement {
  const update = useUpdatePod(workspaceSlug);
  return (
    <Dialog
      open={pod !== null}
      onOpenChange={(next) => {
        if (!next && !update.isPending) onClose();
      }}
      title={EDIT_POD_TITLE}
      description={EDIT_POD_HINT}
      nonDismissable={update.isPending}
      bodyClassName="mt-(--space-4)"
    >
      {pod === null ? null : <EditPodForm pod={pod} update={update} onClose={onClose} />}
    </Dialog>
  );
}
