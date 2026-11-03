// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pods section of the runners overview (NEWFRONT-542, RUN-012). One tile
// per pod in the current scope shows its name, whether it is the project
// default and how many runners sit on it; pressing a tile narrows the
// runner table, pressing it again widens the table. Each tile also opens a
// small menu with its edit and delete entries, and the last tile creates a
// new pod. A failed pod read replaces the tiles with one message.

import { Button, IconButton, Menu } from "@pidash/kit";
import { MoreHorizontal, Pencil, Plus, Trash2 } from "lucide-react";
import * as React from "react";

import type { Pod } from "@pidash/api-client";

export const PODS_SECTION_TITLE = "Pods";
export const PODS_SECTION_HINT =
  "Pods group runners by project. Issues assigned to a pod are picked up by a free runner on it. Press a pod to show only its runners.";
export const PODS_LOAD_FAILED = "Failed to load pods";
export const PODS_EMPTY = "No pods in this scope yet. Create one to group runners under a project.";
export const PODS_CREATE_LABEL = "Create new pod";
export const PODS_EMPTY_RUNNERS_LABEL = "No runners";

/** The count line under a pod's name, e.g. "3 runner(s)". */
export function podRunnerCountLabel(count: number): string {
  return `${count} runner(s)`;
}

/** Accessible name of the tile, shared with the parity driver. */
export function podTileLabel(name: string): string {
  return `Filter runners by pod ${name}`;
}

export interface RunnersPodsProps {
  pods: readonly Pod[] | undefined;
  loading: boolean;
  failed: boolean;
  selectedPodId: string | null;
  onToggle: (podId: string) => void;
  onCreate: () => void;
  onEdit: (pod: Pod) => void;
  onDelete: (pod: Pod) => void;
  /** Disabled until the workspace resolves. */
  createDisabled?: boolean | undefined;
}

function PodTile({
  pod,
  selected,
  onToggle,
  onEdit,
  onDelete,
}: {
  pod: Pod;
  selected: boolean;
  onToggle: () => void;
  onEdit: () => void;
  onDelete: () => void;
}): React.ReactElement {
  return (
    <div className="relative flex min-w-48 items-stretch">
      <button
        type="button"
        aria-pressed={selected}
        aria-label={podTileLabel(pod.name)}
        onClick={onToggle}
        className={`text-meta flex w-full flex-col items-start gap-(--space-1) rounded-(--radius-control) border px-(--space-3) py-(--space-2) pr-(--space-10) text-left hover:border-(--text-muted) ${
          selected ? "border-(--accent) bg-(--subtle)" : "border-(--border) bg-(--bg)"
        }`}
      >
        <span className="font-medium text-(--text)">{pod.name}</span>
        <span className="flex flex-wrap items-center gap-(--space-2) text-(--text-muted)">
          {pod.is_default ? (
            <span className="rounded-(--radius-control) border border-(--border) px-(--space-2)">default</span>
          ) : null}
          <span>{podRunnerCountLabel(pod.runner_count)}</span>
        </span>
        {pod.description === "" ? null : <span className="text-(--text-muted)">{pod.description}</span>}
      </button>
      <div className="absolute top-(--space-2) right-(--space-1)">
        <Menu
          trigger={
            <IconButton size="small" label={`Pod actions for ${pod.name}`}>
              <MoreHorizontal size={14} />
            </IconButton>
          }
          items={[
            { label: "Edit", icon: <Pencil size={13} />, onSelect: onEdit },
            { label: "Delete", icon: <Trash2 size={13} />, onSelect: onDelete },
          ]}
        />
      </div>
    </div>
  );
}

export function RunnersPods({
  pods,
  loading,
  failed,
  selectedPodId,
  onToggle,
  onCreate,
  onEdit,
  onDelete,
  createDisabled = false,
}: RunnersPodsProps): React.ReactElement {
  const rows = pods ?? [];
  return (
    <section aria-label={PODS_SECTION_TITLE} className="flex flex-col gap-(--space-3)">
      <div className="flex flex-col gap-(--space-1)">
        <h2 className="text-emphasis font-medium text-(--text)">{PODS_SECTION_TITLE}</h2>
        <p className="text-meta text-(--text-muted)">{PODS_SECTION_HINT}</p>
      </div>
      {failed ? (
        <p role="alert" className="text-meta text-(--danger)">
          {PODS_LOAD_FAILED}
        </p>
      ) : (
        <div className="flex flex-wrap items-start gap-(--space-2)">
          {rows.map((pod) => (
            <PodTile
              key={pod.id}
              pod={pod}
              selected={pod.id === selectedPodId}
              onToggle={() => onToggle(pod.id)}
              onEdit={() => onEdit(pod)}
              onDelete={() => onDelete(pod)}
            />
          ))}
          {loading && rows.length === 0 ? null : (
            <Button variant="secondary" disabled={createDisabled} onClick={onCreate}>
              <Plus size={14} />
              {PODS_CREATE_LABEL}
            </Button>
          )}
        </div>
      )}
      {!failed && !loading && rows.length === 0 ? <p className="text-meta text-(--text-muted)">{PODS_EMPTY}</p> : null}
    </section>
  );
}
