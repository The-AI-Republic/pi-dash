// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared create/edit dialog for scheduler definitions (AGT-002/003/005).
// The project-side author flow reuses this public component in NEWFRONT-584.

import type { Scheduler } from "@pidash/api-client";
import { Button, Dialog, Input, Switch, Textarea } from "@pidash/kit";
import * as React from "react";

import { schedulerFieldError, useCreateScheduler, useUpdateScheduler } from "./api.js";
import {
  deriveSchedulerHandle,
  emptySchedulerDefinition,
  hasSchedulerDefinitionErrors,
  validateSchedulerDefinition,
  type SchedulerDefinitionErrors,
  type SchedulerDefinitionValue,
} from "./definition.js";

export interface SchedulerDefinitionDialogProps {
  workspaceSlug: string;
  open: boolean;
  scheduler?: Scheduler | null | undefined;
  onClose: () => void;
}

function valueFromScheduler(scheduler: Scheduler | null): SchedulerDefinitionValue {
  if (scheduler === null) return emptySchedulerDefinition();
  return {
    name: scheduler.name,
    slug: scheduler.slug,
    description: scheduler.description,
    prompt: scheduler.prompt,
    color: scheduler.color,
    isEnabled: scheduler.is_enabled,
  };
}

function mergeServerErrors(error: unknown, clientErrors: SchedulerDefinitionErrors): SchedulerDefinitionErrors {
  const merged = { ...clientErrors };
  const fields: Array<[keyof SchedulerDefinitionValue, string]> = [
    ["name", "name"],
    ["slug", "slug"],
    ["description", "description"],
    ["prompt", "prompt"],
    ["color", "color"],
    ["isEnabled", "is_enabled"],
  ];
  for (const [local, remote] of fields) {
    const message = schedulerFieldError(error, remote);
    if (merged[local] === undefined && message !== undefined) merged[local] = message;
  }
  return merged;
}

function errorProp(error: string | undefined): { error?: string } {
  return error === undefined ? {} : { error };
}

interface SchedulerDefinitionFormProps {
  workspaceSlug: string;
  scheduler: Scheduler | null;
  onClose: () => void;
}

function SchedulerDefinitionForm({
  workspaceSlug,
  scheduler,
  onClose,
}: SchedulerDefinitionFormProps): React.ReactElement {
  const editing = scheduler !== null;
  const create = useCreateScheduler(workspaceSlug);
  const update = useUpdateScheduler(workspaceSlug);
  const mutation = editing ? update : create;
  const [value, setValue] = React.useState(() => valueFromScheduler(scheduler));
  const [handleEdited, setHandleEdited] = React.useState(false);
  const [clientErrors, setClientErrors] = React.useState<SchedulerDefinitionErrors>({});
  const errors = mergeServerErrors(mutation.error, clientErrors);

  function change<K extends keyof SchedulerDefinitionValue>(field: K, next: SchedulerDefinitionValue[K]): void {
    setValue((current) => ({ ...current, [field]: next }));
    setClientErrors((current) => ({ ...current, [field]: undefined }));
  }

  async function submit(): Promise<void> {
    const nextErrors = validateSchedulerDefinition(value);
    setClientErrors(nextErrors);
    if (hasSchedulerDefinitionErrors(nextErrors)) return;
    if (scheduler === null) {
      await create.mutateAsync({
        name: value.name.trim(),
        slug: value.slug,
        description: value.description,
        prompt: value.prompt,
        color: value.color,
        is_enabled: value.isEnabled,
      });
    } else {
      await update.mutateAsync({
        schedulerId: scheduler.id,
        input: {
          name: value.name.trim(),
          description: value.description,
          prompt: value.prompt,
          color: value.color,
          is_enabled: value.isEnabled,
        },
      });
    }
    onClose();
  }

  return (
    <form
      aria-label={editing ? "Edit scheduler" : "Create scheduler"}
      className="flex flex-col gap-(--space-5)"
      onSubmit={(event) => {
        event.preventDefault();
        void submit().catch(() => undefined);
      }}
    >
      <Input
        label="Name"
        value={value.name}
        disabled={mutation.isPending}
        {...errorProp(errors.name)}
        autoFocus
        onChange={(event) => {
          const name = event.target.value;
          change("name", name);
          if (!editing && !handleEdited) change("slug", deriveSchedulerHandle(name));
        }}
      />
      <Input
        label="Handle"
        value={value.slug}
        disabled={editing || mutation.isPending}
        {...errorProp(errors.slug)}
        hint={editing ? "Handles cannot change after creation." : "Used in scheduler links."}
        autoComplete="off"
        onChange={(event) => {
          setHandleEdited(true);
          change("slug", event.target.value.toLowerCase());
        }}
      />
      <Textarea
        label="Description"
        value={value.description}
        disabled={mutation.isPending}
        {...errorProp(errors.description)}
        rows={3}
        onChange={(event) => change("description", event.target.value)}
      />
      <Textarea
        label="Base prompt"
        value={value.prompt}
        disabled={mutation.isPending}
        {...errorProp(errors.prompt)}
        rows={6}
        onChange={(event) => change("prompt", event.target.value)}
      />
      <Input
        label="Color"
        type="color"
        value={value.color}
        disabled={mutation.isPending}
        {...errorProp(errors.color)}
        onChange={(event) => change("color", event.target.value)}
      />
      <div className="flex items-center justify-between gap-(--space-4) rounded-(--radius-control) border border-(--border) p-(--space-4)">
        <div>
          <p className="text-body font-medium text-(--text)">Enabled</p>
          <p className="text-meta text-(--text-muted)">Allow project installs to fire this definition.</p>
          {errors.isEnabled ? <p className="text-meta text-(--danger)">{errors.isEnabled}</p> : null}
        </div>
        <Switch
          label="Scheduler enabled"
          checked={value.isEnabled}
          disabled={mutation.isPending}
          onCheckedChange={(checked) => change("isEnabled", checked)}
        />
      </div>
      <div className="flex justify-end gap-(--space-3)">
        <Button variant="ghost" disabled={mutation.isPending} onClick={onClose}>
          Cancel
        </Button>
        <Button variant="primary" type="submit" loading={mutation.isPending}>
          {editing ? "Save scheduler" : "Create scheduler"}
        </Button>
      </div>
    </form>
  );
}

export function SchedulerDefinitionDialog({
  workspaceSlug,
  open,
  scheduler = null,
  onClose,
}: SchedulerDefinitionDialogProps): React.ReactElement {
  const editing = scheduler !== null;
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
      nonDismissable={false}
      title={editing ? "Edit scheduler" : "Create scheduler"}
      description={editing ? "Update the reusable definition." : "Create a definition projects can install."}
    >
      {open ? (
        <SchedulerDefinitionForm
          key={scheduler?.id ?? "create"}
          workspaceSlug={workspaceSlug}
          scheduler={scheduler}
          onClose={onClose}
        />
      ) : null}
    </Dialog>
  );
}
