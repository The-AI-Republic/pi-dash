// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Execution-target picker (parity/specs/issues.md §17, ISS-218).
// Single-select over the offered cloud/managed agent rows plus each
// pod's squad row and its runners, with search. Unavailable agents
// and unassignable runners disable with their reason inline; the
// default pod carries a tag. Picks map onto the issue execution-target
// patch. Reads resolve lazily on open; a caller that has to name an
// existing target before the popup opens preloads them.
import { Badge, Button, Combobox, IconButton, type ButtonVariant, type ComboboxOption } from "@pidash/kit";
import { Cloud, Container, Monitor, Server } from "lucide-react";
import * as React from "react";

import { useWorkspace } from "../../core/session/hooks.js";
import {
  buildExecutionTargets,
  executionSelectionId,
  isAgentRowKind,
  parseExecutionSelectionId,
  podOptionLabel,
  selectRunnerOwnerName,
  selectRunnerWaitNote,
  selectTriggerPodName,
  selectTriggerText,
  type AgentRowKind,
  type ExecutionSelection,
  type ExecutionTarget,
} from "./pods.js";
import { usePickerPods, usePickerProject, usePickerRunners, usePickerWorkspaceMembers } from "./reference.js";

export interface PodPickerProps {
  workspaceSlug: string;
  projectId: string;
  value: ExecutionSelection | null;
  onChange: (selection: ExecutionSelection) => void;
  disabled?: boolean;
  /** Button variant of the default trigger. */
  variant?: ButtonVariant;
  /** Default trigger shows the glyph without the name. */
  iconOnly?: boolean;
  /** Fully custom trigger; overrides the default value button. */
  trigger?: React.ReactElement<Record<string, unknown>>;
  /** Default trigger text when nothing is selected. */
  placeholder?: string;
  /** Read targets up front so a set selection names itself before the popup opens. */
  preload?: boolean;
  open?: boolean | undefined;
  onOpenChange?: ((open: boolean) => void) | undefined;
  /**
   * Eligibility scope for the runner rows: the issue id, or "new" on
   * the create form.
   */
  assignableFor?: string;
  /** Compact waiting marker on the trigger (the dispatch-wait reason). */
  waitingLabel?: string | undefined;
  /** API validation message, shown in place under the trigger. */
  saveError?: string | null | undefined;
}

function PodGlyph(): React.ReactElement {
  return <Container size={16} strokeWidth={1.5} className="text-(--text-muted)" aria-hidden="true" />;
}

function RunnerGlyph(): React.ReactElement {
  return <Server size={16} strokeWidth={1.5} className="text-(--text-muted)" aria-hidden="true" />;
}

function AgentGlyph({ executor }: { executor: AgentRowKind }): React.ReactElement {
  const Icon = executor === "cloud_agent" ? Cloud : Monitor;
  return <Icon size={16} strokeWidth={1.5} className="text-(--text-muted)" aria-hidden="true" />;
}

function DefaultPill(): React.ReactElement {
  return (
    <span className="text-caption rounded-(--radius-full) bg-(--subtle) px-(--space-2) py-px text-(--text-muted)">
      DEFAULT
    </span>
  );
}

function ReasonText({ reason }: { reason: string }): React.ReactElement {
  return <span className="text-caption whitespace-nowrap text-(--text-muted)">{reason}</span>;
}

function toComboboxOption(target: ExecutionTarget): ComboboxOption {
  if (target.kind === "agent") {
    return {
      id: `agent:${target.executor}`,
      label: target.label,
      disabled: !target.available,
      icon: <AgentGlyph executor={target.executor} />,
      trailing: target.reason === "" ? undefined : <ReasonText reason={target.reason} />,
    };
  }
  if (target.kind === "pod") {
    return {
      id: `pod:${target.podId}`,
      label: podOptionLabel(target.label),
      searchText: target.isDefault ? `${target.label} default` : target.label,
      icon: <PodGlyph />,
      trailing: target.isDefault ? <DefaultPill /> : undefined,
    };
  }
  const caption = target.ownerName === "" ? target.statusLine : `${target.statusLine} · ${target.ownerName}`;
  return {
    id: `runner:${target.runnerId}`,
    label: target.label,
    searchText: `${target.label} ${target.statusLine} ${target.ownerName}`,
    disabled: !target.assignable,
    content: (
      <span className="pl-(--space-6)">
        {target.label} <span className="text-caption text-(--text-muted)">· {caption}</span>
      </span>
    ),
    trailing: target.reason === "" ? undefined : <ReasonText reason={target.reason} />,
  };
}

function selectionGlyph(selection: ExecutionSelection | null, targets: ExecutionTarget[]): React.ReactNode {
  if (selection === null) return null;
  if (selection.type === "agent") {
    return isAgentRowKind(selection.executor) ? <AgentGlyph executor={selection.executor} /> : null;
  }
  if (selection.type === "pod") {
    const pod = targets.find((target) => target.kind === "pod" && target.podId === selection.podId);
    return pod === undefined ? null : <PodGlyph />;
  }
  const runner = targets.find((target) => target.kind === "runner" && target.runnerId === selection.runnerId);
  return runner === undefined ? null : <RunnerGlyph />;
}

export function PodPicker({
  workspaceSlug,
  projectId,
  value,
  onChange,
  disabled = false,
  variant = "secondary",
  iconOnly = false,
  trigger,
  placeholder = "Select target",
  preload = false,
  open: controlledOpen,
  onOpenChange,
  assignableFor = "new",
  waitingLabel,
  saveError,
}: PodPickerProps): React.ReactElement {
  const [uncontrolledOpen, setUncontrolledOpen] = React.useState(false);
  const open = controlledOpen ?? uncontrolledOpen;

  function setOpen(next: boolean): void {
    if (controlledOpen === undefined) setUncontrolledOpen(next);
    onOpenChange?.(next);
  }

  const readsAllowed = open || preload;
  const { workspace } = useWorkspace(workspaceSlug);
  const workspaceId = workspace?.id ?? "";
  const pods = usePickerPods(workspaceSlug, projectId, readsAllowed);
  const project = usePickerProject(workspaceSlug, projectId, readsAllowed);
  const runners = usePickerRunners(workspaceSlug, workspaceId, projectId, assignableFor, readsAllowed);
  // The directory only names runner owners, so it stays off until a
  // runner row needs it; pod-only projects never pay for the read.
  const directory = usePickerWorkspaceMembers(workspaceSlug, readsAllowed && (runners.data?.length ?? 0) > 0);
  const targets = React.useMemo(
    () =>
      buildExecutionTargets(
        pods.data ?? [],
        project.data?.agent_executor_options ?? [],
        runners.data ?? [],
        (ownerId) => selectRunnerOwnerName(directory.data, ownerId)
      ),
    [pods.data, project.data, runners.data, directory.data]
  );
  // Without a workspace id the runner read stays off and the picker
  // offers pods and agents alone; it must neither spin nor wait for it.
  const loading =
    open && !disabled && (pods.isPending || project.isPending || (workspaceId !== "" && runners.isPending));
  const errorText = pods.isError || project.isError || runners.isError ? "Couldn't load execution targets." : undefined;

  function handleSelect(id: string): void {
    onChange(parseExecutionSelectionId(id, runners.data ?? []));
  }

  const listsLoaded = pods.data !== undefined && (workspaceId === "" || runners.data !== undefined);
  const waitNote =
    value !== null && value.type === "runner" ? selectRunnerWaitNote(runners.data, value.runnerId) : null;

  function defaultTrigger(): React.ReactElement<Record<string, unknown>> {
    const text = selectTriggerText(value, targets, listsLoaded, placeholder);
    const glyph = selectionGlyph(value, targets);
    if (iconOnly) {
      return (
        <IconButton label={text} variant={variant}>
          {glyph}
        </IconButton>
      );
    }
    const podName = selectTriggerPodName(value, targets);
    return (
      <Button variant={variant}>
        {glyph === null ? null : (
          <span aria-hidden="true" className="inline-flex shrink-0 items-center">
            {glyph}
          </span>
        )}
        <span className="truncate">{text}</span>
        {podName === null ? null : <span className="text-caption truncate text-(--text-muted)">{podName}</span>}
        {waitingLabel === undefined ? null : (
          <Badge tone="warning" dot>
            {waitingLabel}
          </Badge>
        )}
      </Button>
    );
  }

  return (
    <div className="flex min-w-0 flex-col gap-(--space-1)">
      <Combobox
        trigger={trigger ?? defaultTrigger()}
        options={targets.map(toComboboxOption)}
        value={executionSelectionId(value)}
        onSelect={handleSelect}
        listLabel="Execution targets"
        searchLabel="Search execution targets"
        emptyText="No matching results"
        loading={loading}
        loadingText="Loading execution targets"
        errorText={errorText}
        disabled={disabled}
        open={open}
        onOpenChange={setOpen}
      />
      {waitNote === null ? null : <p className="text-caption text-(--text-muted)">{waitNote}</p>}
      {saveError === undefined || saveError === null ? null : (
        <p role="alert" className="text-caption text-(--danger)">
          {saveError}
        </p>
      )}
    </div>
  );
}
