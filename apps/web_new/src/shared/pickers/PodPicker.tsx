// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Execution-target picker (parity/specs/issues.md §17, ISS-218).
// Single-select over the offered cloud/managed agent rows plus the
// project's pods with search. Unavailable agents disable with their
// reason inline; the default pod carries a tag. Picks map onto the
// issue execution-target patch. Reads resolve lazily on open.
import { Button, Combobox, IconButton, type ButtonVariant, type ComboboxOption } from "@pidash/kit";
import { Cloud, Container, Monitor } from "lucide-react";
import * as React from "react";

import {
  AGENT_ROW_LABELS,
  buildExecutionTargets,
  executionSelectionId,
  isAgentRowKind,
  parseExecutionSelectionId,
  type AgentRowKind,
  type ExecutionSelection,
  type ExecutionTarget,
} from "./pods.js";
import { usePickerPods, usePickerProject } from "./reference.js";

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
  open?: boolean | undefined;
  onOpenChange?: ((open: boolean) => void) | undefined;
}

function PodGlyph(): React.ReactElement {
  return <Container size={16} strokeWidth={1.5} className="text-(--text-muted)" aria-hidden="true" />;
}

function AgentGlyph({ executor }: { executor: AgentRowKind }): React.ReactElement {
  const Icon = executor === "cloud_agent" ? Cloud : Monitor;
  return <Icon size={16} strokeWidth={1.5} className="text-(--text-muted)" aria-hidden="true" />;
}

function TargetGlyph({ target }: { target: ExecutionTarget }): React.ReactElement {
  return target.kind === "pod" ? <PodGlyph /> : <AgentGlyph executor={target.executor} />;
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
  return {
    id: `pod:${target.podId}`,
    label: target.label,
    searchText: target.isDefault ? `${target.label} default` : target.label,
    icon: <PodGlyph />,
    trailing: target.isDefault ? <DefaultPill /> : undefined,
  };
}

/**
 * Trigger text for a selection: the pod or agent name. A set pod
 * renders the prompt until the pods load; once loaded, a missing pod
 * reads as unknown (every single picker shares the prompt fallback).
 */
function selectionText(
  selection: ExecutionSelection | null,
  targets: ExecutionTarget[],
  podsLoaded: boolean,
  placeholder: string
): string {
  if (selection === null) return placeholder;
  if (selection.type === "agent") {
    return isAgentRowKind(selection.executor) ? AGENT_ROW_LABELS[selection.executor] : placeholder;
  }
  if (!podsLoaded) return placeholder;
  const pod = targets.find((target) => target.kind === "pod" && target.podId === selection.podId);
  return pod?.label ?? "Unknown pod";
}

function selectionGlyph(selection: ExecutionSelection | null, targets: ExecutionTarget[]): React.ReactNode {
  if (selection === null) return null;
  if (selection.type === "agent") {
    return isAgentRowKind(selection.executor) ? <AgentGlyph executor={selection.executor} /> : null;
  }
  const pod = targets.find((target) => target.kind === "pod" && target.podId === selection.podId);
  return pod === undefined ? null : <TargetGlyph target={pod} />;
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
  open: controlledOpen,
  onOpenChange,
}: PodPickerProps): React.ReactElement {
  const [uncontrolledOpen, setUncontrolledOpen] = React.useState(false);
  const open = controlledOpen ?? uncontrolledOpen;

  function setOpen(next: boolean): void {
    if (controlledOpen === undefined) setUncontrolledOpen(next);
    onOpenChange?.(next);
  }

  const pods = usePickerPods(workspaceSlug, projectId, open && !disabled);
  const project = usePickerProject(workspaceSlug, projectId, open && !disabled);
  const targets = React.useMemo(
    () => buildExecutionTargets(pods.data ?? [], project.data?.agent_executor_options ?? []),
    [pods.data, project.data]
  );
  const loading = (open && !disabled && pods.isPending) || (open && !disabled && project.isPending);
  const errorText = pods.isError || project.isError ? "Couldn't load pods." : undefined;

  function handleSelect(id: string): void {
    onChange(parseExecutionSelectionId(id));
  }

  function defaultTrigger(): React.ReactElement<Record<string, unknown>> {
    const text = selectionText(value, targets, pods.data !== undefined, placeholder);
    const glyph = selectionGlyph(value, targets);
    if (iconOnly) {
      return (
        <IconButton label={text} variant={variant}>
          {glyph}
        </IconButton>
      );
    }
    return (
      <Button variant={variant}>
        {glyph === null ? null : (
          <span aria-hidden="true" className="inline-flex shrink-0 items-center">
            {glyph}
          </span>
        )}
        <span className="truncate">{text}</span>
      </Button>
    );
  }

  return (
    <Combobox
      trigger={trigger ?? defaultTrigger()}
      options={targets.map(toComboboxOption)}
      value={executionSelectionId(value)}
      onSelect={handleSelect}
      listLabel="Execution targets"
      searchLabel="Search execution targets"
      emptyText="No matching results"
      loading={loading}
      loadingText="Loading pods"
      errorText={errorText}
      disabled={disabled}
      open={open}
      onOpenChange={setOpen}
    />
  );
}
