// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Quick-create dialog (parity/specs/shell.md §4, SHELL-047). The shared
// work-item creation entry point: the sidebar button and (once the
// issues epic registers it) the palette command open this same form.
// Title plus target project, kept as a per-workspace draft until the
// submit lands. The issues epic extends the field set in place.

import { Button, Dialog, Input, Menu } from "@pidash/kit";
import { Check, ChevronDown } from "lucide-react";
import * as React from "react";

import { useCreateWorkItem } from "../api/create.js";
import { clearWorkItemDraft, loadWorkItemDraft, storeWorkItemDraft } from "../lib/createDraft.js";

/** Joined project the dialog can file the work item under. */
export interface WorkItemCreateProject {
  id: string;
  name: string;
}

export interface WorkItemCreateDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  workspaceSlug: string;
  /** Joined projects; empty renders the no-target notice. */
  projects: WorkItemCreateProject[];
}

const TITLE_REQUIRED = "Give the work item a title.";

function initialTitle(workspaceSlug: string): string {
  return loadWorkItemDraft(workspaceSlug)?.title ?? "";
}

function initialProjectId(workspaceSlug: string, projects: WorkItemCreateProject[]): string | null {
  const wanted = loadWorkItemDraft(workspaceSlug)?.projectId ?? null;
  if (wanted !== null && projects.some((project) => project.id === wanted)) return wanted;
  return projects[0]?.id ?? null;
}

export function WorkItemCreateDialog({
  open,
  onOpenChange,
  workspaceSlug,
  projects,
}: WorkItemCreateDialogProps): React.ReactElement {
  const create = useCreateWorkItem(workspaceSlug);
  const [title, setTitle] = React.useState(() => initialTitle(workspaceSlug));
  const [projectId, setProjectId] = React.useState<string | null>(() => initialProjectId(workspaceSlug, projects));
  const [titleInvalid, setTitleInvalid] = React.useState(false);
  const selected = projects.find((project) => project.id === projectId) ?? null;

  function updateTitle(value: string): void {
    setTitle(value);
    setTitleInvalid(false);
    storeWorkItemDraft(workspaceSlug, { title: value, projectId });
  }

  function pickProject(id: string): void {
    setProjectId(id);
    storeWorkItemDraft(workspaceSlug, { title, projectId: id });
  }

  function submit(): void {
    const name = title.trim();
    if (name === "") {
      setTitleInvalid(true);
      return;
    }
    // Defensive: the form renders only with a target project, so the
    // picker always holds one; the guard keeps the type honest.
    if (projectId === null) return;
    create.mutate(
      { projectId, name },
      {
        onSuccess: () => {
          clearWorkItemDraft(workspaceSlug);
          setTitle("");
          setTitleInvalid(false);
          onOpenChange(false);
        },
      }
    );
  }

  return (
    <Dialog
      title="Create new work item"
      description="File it under one of your projects."
      open={open}
      onOpenChange={onOpenChange}
      actions={
        <>
          <Button variant="secondary" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          {projects.length === 0 ? null : (
            <Button variant="primary" onClick={submit} disabled={create.isPending}>
              Create work item
            </Button>
          )}
        </>
      }
    >
      {projects.length === 0 ? (
        <p className="text-body text-(--text-muted)">Join a project first — new work items need a home.</p>
      ) : (
        <div className="flex flex-col gap-(--space-4)">
          <Menu
            trigger={
              <Button variant="secondary" aria-label="Target project">
                <span className="truncate">{selected?.name ?? "Pick a project"}</span>
                <ChevronDown size={14} strokeWidth={2} aria-hidden="true" />
              </Button>
            }
            items={projects.map((project) => ({
              label: project.name,
              itemKey: project.id,
              icon: project.id === projectId ? <Check size={14} strokeWidth={2} aria-hidden="true" /> : undefined,
              onSelect: () => pickProject(project.id),
            }))}
          />
          <Input
            label="Title"
            value={title}
            maxLength={255}
            onChange={(event) => updateTitle(event.target.value)}
            {...(titleInvalid ? { error: TITLE_REQUIRED } : {})}
          />
        </div>
      )}
    </Dialog>
  );
}
