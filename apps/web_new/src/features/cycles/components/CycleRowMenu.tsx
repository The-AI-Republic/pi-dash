// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One cycle row's action menu (parity/specs/cycles.md §10, CYC-028).
// Entries come from the menu model in spec order — edit, the link
// actions, then the mutations behind a separator — gated by role and
// cycle state. Edit opens the shared save dialog, the link actions
// call the shared helpers, archive and delete confirm through their
// dialogs, and restore runs straight from the menu. The trigger
// reveals on row hover on desktop and stays visible on touch; the
// titles render as headings with the gate hint beneath so the parity
// driver reads them the same way on both apps.

import { IconButton, Menu, type MenuEntry as KitMenuEntry } from "@pidash/kit";
import type { CycleRow as CycleRowData } from "@pidash/api-client";
import { Archive, ArchiveRestore, Ellipsis, ExternalLink, Link2, Pencil, Trash2 } from "lucide-react";
import * as React from "react";

import { UpdateCycleDialog, type CycleProjectRef } from "../dialog/CycleDialog.js";
import { useArchiveCycle, useDeleteCycle, useRestoreCycle } from "../api/cycleMutations.js";
import { cycleMenuEntries, type CycleMenuEntryId } from "../lib/cycleMenu.js";
import { copyCycleLink, openCycleInNewTab } from "../lib/cycleShare.js";
import { CycleArchiveDialog } from "./CycleArchiveDialog.js";
import { CycleDeleteDialog } from "./CycleDeleteDialog.js";

export interface CycleRowMenuProps {
  workspaceSlug: string;
  projectId: string;
  cycle: CycleRowData;
  /** Members and admins mutate; guests keep the link actions only. */
  canMutate: boolean;
  /** The project in the address: the edit dialog's pre-read label. */
  currentProject: CycleProjectRef;
}

const ENTRY_ICONS: Record<CycleMenuEntryId, React.ReactNode> = {
  edit: <Pencil size={16} strokeWidth={1.5} aria-hidden="true" />,
  open: <ExternalLink size={16} strokeWidth={1.5} aria-hidden="true" />,
  copy: <Link2 size={16} strokeWidth={1.5} aria-hidden="true" />,
  archive: <Archive size={16} strokeWidth={1.5} aria-hidden="true" />,
  restore: <ArchiveRestore size={16} strokeWidth={1.5} aria-hidden="true" />,
  delete: <Trash2 size={16} strokeWidth={1.5} aria-hidden="true" />,
};

/** The mutation group sits behind a separator from the navigation entries. */
function isMutationEntry(id: CycleMenuEntryId): boolean {
  return id === "archive" || id === "restore" || id === "delete";
}

export function CycleRowMenu({
  workspaceSlug,
  projectId,
  cycle,
  canMutate,
  currentProject,
}: CycleRowMenuProps): React.ReactElement {
  const [editOpen, setEditOpen] = React.useState(false);
  const [deleteOpen, setDeleteOpen] = React.useState(false);
  const [archiveOpen, setArchiveOpen] = React.useState(false);
  const target = { workspaceSlug, projectId, cycleId: cycle.id };
  const { remove, pending: deletePending } = useDeleteCycle(target);
  const { archive, pending: archivePending } = useArchiveCycle(target);
  const { restore } = useRestoreCycle(target);

  const entries = cycleMenuEntries(cycle, canMutate);
  const items: KitMenuEntry[] = [];
  let previous: CycleMenuEntryId | null = null;
  for (const entry of entries) {
    if (previous !== null && !isMutationEntry(previous) && isMutationEntry(entry.id)) {
      items.push({ separator: true });
    }
    previous = entry.id;
    const onSelect = (): void => {
      if (entry.id === "edit") setEditOpen(true);
      else if (entry.id === "open") void openCycleInNewTab(workspaceSlug, projectId, cycle.id);
      else if (entry.id === "copy") void copyCycleLink(workspaceSlug, projectId, cycle.id);
      else if (entry.id === "archive") setArchiveOpen(true);
      else if (entry.id === "restore") void restore();
      else setDeleteOpen(true);
    };
    items.push({
      label: entry.title,
      onSelect,
      disabled: entry.disabled,
      icon: ENTRY_ICONS[entry.id],
      content: (
        <>
          <h5 className="text-body font-medium text-(--text)">{entry.title}</h5>
          {entry.hint === null ? null : <p className="text-caption text-(--text-muted)">{entry.hint}</p>}
        </>
      ),
    });
  }

  return (
    <>
      <Menu
        trigger={
          <IconButton
            label={`Cycle actions for ${cycle.name}`}
            size="small"
            data-testid="cycle-row-menu-trigger"
            className="shrink-0 opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100 pointer-coarse:opacity-100"
          >
            <Ellipsis size={16} strokeWidth={1.5} aria-hidden="true" />
          </IconButton>
        }
        items={items}
      />
      <UpdateCycleDialog
        open={editOpen}
        onOpenChange={setEditOpen}
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        currentProject={currentProject}
        cycle={cycle}
      />
      <CycleDeleteDialog
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        cycleName={cycle.name}
        pending={deletePending}
        onConfirm={() => {
          void remove().then((ok) => {
            if (ok) setDeleteOpen(false);
          });
        }}
      />
      <CycleArchiveDialog
        open={archiveOpen}
        onOpenChange={setArchiveOpen}
        cycleName={cycle.name}
        pending={archivePending}
        onConfirm={() => {
          void archive().then((ok) => {
            if (ok) setArchiveOpen(false);
          });
        }}
      />
    </>
  );
}
