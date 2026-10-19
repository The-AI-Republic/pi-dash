// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// New-pod dialog (NEWFRONT-542, RUN-013). A pod lives under one project and
// its stored name starts with that project's identifier, so the dialog asks
// for the project and the bare suffix and lets the server put the prefix
// back. The form mounts fresh on every open, which both resets its values
// and holds the project read until the dialog is actually needed.

import { Button, Combobox, Dialog, Input } from "@pidash/kit";
import { ChevronDown } from "lucide-react";
import * as React from "react";

import { notifyError } from "../../../shared/shell/index.js";
import { useCreatePod, usePodProjects } from "../api/pods.js";
import {
  POD_CREATE_FAILED,
  POD_ERROR_TITLE,
  podWriteErrorMessage,
  validatePodDraft,
  type PodDraftErrors,
} from "../pods.js";

export const CREATE_POD_TITLE = "Create new pod";
export const CREATE_POD_HINT =
  "A pod gathers runners under one project. Choose the project, then name the pod; the project's identifier is added in front of the name.";
export const CREATE_POD_PROJECT_PLACEHOLDER = "Select a project";
export const CREATE_POD_PROJECT_HINT = "The pod belongs to this project.";
export const CREATE_POD_NAME_HINT =
  "Letters, digits, dashes and underscores. The project identifier is prefixed for you.";
export const CREATE_POD_DESCRIPTION_LABEL = "Description (optional)";
export const CREATE_POD_SUBMIT = "Create pod";
export const CREATE_POD_SUBMITTING = "Creating…";
export const CREATE_POD_PROJECTS_FAILED = "Could not load projects.";

export interface CreatePodDialogProps {
  workspaceSlug: string;
  open: boolean;
  onClose: () => void;
}

function CreatePodForm({
  workspaceSlug,
  create,
  onClose,
}: {
  workspaceSlug: string;
  create: ReturnType<typeof useCreatePod>;
  onClose: () => void;
}): React.ReactElement {
  const projects = usePodProjects(workspaceSlug);
  const [projectId, setProjectId] = React.useState("");
  const [name, setName] = React.useState("");
  const [description, setDescription] = React.useState("");
  const [errors, setErrors] = React.useState<PodDraftErrors>({});

  const rows = projects.data ?? [];
  const pickerDisabled = projects.isPending || projects.isError || rows.length === 0;
  const chosen = rows.find((project) => project.id === projectId) ?? null;

  function submit(event: React.FormEvent): void {
    event.preventDefault();
    const found = validatePodDraft({ projectId, name });
    setErrors(found);
    if (Object.keys(found).length > 0) return;
    create.mutate(
      {
        project: projectId,
        name: name.trim(),
        ...(description.trim() === "" ? {} : { description: description.trim() }),
      },
      {
        onSuccess: onClose,
        onError: (error) => notifyError(POD_ERROR_TITLE, podWriteErrorMessage(error, POD_CREATE_FAILED)),
      }
    );
  }

  return (
    <form className="flex flex-col gap-(--space-5)" noValidate onSubmit={submit}>
      <div className="flex flex-col gap-(--space-1)">
        <span className="text-body font-medium text-(--text)">Project</span>
        <Combobox
          trigger={
            <Button variant="secondary" className="w-full justify-between">
              <span>{chosen?.name ?? CREATE_POD_PROJECT_PLACEHOLDER}</span>
              <ChevronDown size={14} />
            </Button>
          }
          options={rows.map((project) => ({ id: project.id, label: project.name }))}
          value={chosen === null ? null : chosen.id}
          onSelect={(id) => {
            setProjectId(id);
            setErrors((current) => ({ ...current, projectId: "" }));
          }}
          listLabel="Projects"
          searchLabel="Find a project"
          emptyText="No projects to choose from"
          loading={projects.isPending}
          loadingText="Loading projects"
          errorText={projects.isError ? CREATE_POD_PROJECTS_FAILED : undefined}
          disabled={pickerDisabled}
        />
        <span className="text-meta text-(--text-muted)">{CREATE_POD_PROJECT_HINT}</span>
        {projects.isError ? (
          <span role="alert" className="text-meta text-(--danger)">
            {CREATE_POD_PROJECTS_FAILED}
          </span>
        ) : null}
        {errors.projectId === undefined || errors.projectId === "" ? null : (
          <span role="alert" className="text-meta text-(--danger)">
            {errors.projectId}
          </span>
        )}
      </div>
      <Input
        label="Name"
        value={name}
        placeholder="beefy"
        hint={CREATE_POD_NAME_HINT}
        {...(errors.name === undefined || errors.name === "" ? {} : { error: errors.name })}
        onChange={(event) => {
          setName(event.target.value);
          setErrors((current) => ({ ...current, name: "" }));
        }}
      />
      <Input
        label={CREATE_POD_DESCRIPTION_LABEL}
        value={description}
        placeholder="Where this pod runs, and what it is for"
        onChange={(event) => setDescription(event.target.value)}
      />
      <div className="flex justify-end gap-(--space-2)">
        <Button variant="ghost" disabled={create.isPending} onClick={onClose}>
          Cancel
        </Button>
        <Button type="submit" variant="primary" loading={create.isPending}>
          {create.isPending ? CREATE_POD_SUBMITTING : CREATE_POD_SUBMIT}
        </Button>
      </div>
    </form>
  );
}

export function CreatePodDialog({ workspaceSlug, open, onClose }: CreatePodDialogProps): React.ReactElement {
  const create = useCreatePod(workspaceSlug);
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next && !create.isPending) onClose();
      }}
      title={CREATE_POD_TITLE}
      description={CREATE_POD_HINT}
      nonDismissable={create.isPending}
      bodyClassName="mt-(--space-4)"
    >
      {open ? <CreatePodForm workspaceSlug={workspaceSlug} create={create} onClose={onClose} /> : null}
    </Dialog>
  );
}
