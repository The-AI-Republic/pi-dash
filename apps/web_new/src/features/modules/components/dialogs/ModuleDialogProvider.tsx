// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The modules area's dialog openers (parity/specs/modules.md §2.3, §3.2,
// §7.1–§7.5, MOD-003/009/022–025). One provider owns the create/update
// dialog, the delete confirmation and the archive confirmation for a
// project, and exposes the openers every entry point uses: the header Add
// action and the zero-state shortcut open create; row, card, peek and
// detail menus open edit, delete or archive. Mounting the provider twice
// for one project is never needed — the screen mounts it once, and a host
// (the detail page) mounts its own around the surface it renders. Slice G
// adds the archive opener (MOD-026): the confirmation it opens archives,
// shows the notice and lands on the modules list.

import type { ModuleRow } from "@pidash/api-client";
import * as React from "react";

import type { ModuleDialogMode } from "../../lib/moduleForm.js";
import { ModuleArchiveDialog, type ModuleArchiveTarget } from "./ModuleArchiveDialog.js";
import { ModuleDeleteDialog } from "./ModuleDeleteDialog.js";
import type { ModuleFormProject } from "./ModuleForm.js";
import { ModuleSaveDialog } from "./ModuleSaveDialog.js";

/** A module the delete confirmation can name and remove. */
export type ModuleDeleteTarget = Pick<ModuleRow, "id" | "name">;

/** What happens after a delete lands, chosen by the surface that opened it. */
export interface ModuleDeleteOptions {
  onDeleted?: ((module: ModuleDeleteTarget) => void) | undefined;
}

/** The openers every module surface calls. */
export interface ModuleDialogHandle {
  /** Create a module; the dialog opens on defaults in this project. */
  openCreate: () => void;
  /** Edit one module; the dialog opens prefilled without a project picker. */
  openEdit: (module: ModuleRow) => void;
  /** Confirm deleting one module. */
  openDelete: (module: ModuleDeleteTarget, options?: ModuleDeleteOptions) => void;
  /**
   * Confirm archiving one finished module. The confirmation archives,
   * shows the notice and lands on the modules list unless the surface
   * supplies its own landing.
   */
  openArchive: (module: ModuleArchiveTarget, options?: ModuleArchiveOptions) => void;
}

/** What happens after an archive lands, chosen by the surface that opened it. */
export interface ModuleArchiveOptions {
  onArchived?: ((module: ModuleArchiveTarget) => void) | undefined;
}

export interface ModuleDialogProviderProps {
  workspaceSlug: string;
  projectId: string;
  /** The project this surface is on; labels the create picker's selection. */
  currentProject: ModuleFormProject;
  children: React.ReactNode;
}

const ModuleDialogContext = React.createContext<ModuleDialogHandle | null>(null);

/**
 * The project's dialog openers. A surface outside the provider has no
 * dialogs to open, so it renders no entry that would call one.
 */
export function useModuleDialogs(): ModuleDialogHandle | null {
  return React.useContext(ModuleDialogContext);
}

type SaveSession = { mode: "create" } | { mode: "update"; module: ModuleRow };

export function ModuleDialogProvider({
  workspaceSlug,
  projectId,
  currentProject,
  children,
}: ModuleDialogProviderProps): React.ReactElement {
  const [session, setSession] = React.useState<SaveSession | null>(null);
  const [deleting, setDeleting] = React.useState<{ target: ModuleDeleteTarget; options: ModuleDeleteOptions } | null>(
    null
  );
  const [archiving, setArchiving] = React.useState<{
    target: ModuleArchiveTarget;
    options: ModuleArchiveOptions;
  } | null>(null);

  const handle = React.useMemo<ModuleDialogHandle>(
    () => ({
      openCreate: () => {
        setSession({ mode: "create" });
      },
      openEdit: (module) => {
        setSession({ mode: "update", module });
      },
      openDelete: (target, options) => {
        setDeleting({ target, options: options ?? {} });
      },
      openArchive: (target, options) => {
        setArchiving({ target, options: options ?? {} });
      },
    }),
    []
  );

  const mode: ModuleDialogMode = session?.mode ?? "create";

  return (
    <ModuleDialogContext.Provider value={handle}>
      {children}
      <ModuleSaveDialog
        open={session !== null}
        onOpenChange={(open) => {
          if (!open) setSession(null);
        }}
        mode={mode}
        workspaceSlug={workspaceSlug}
        currentProject={currentProject}
        module={session?.mode === "update" ? session.module : undefined}
      />
      {/* Remounted per target so a cancel never leaves the previous
          module's submitting state behind. */}
      <ModuleArchiveDialog
        key={archiving?.target.id ?? "closed"}
        module={archiving?.target ?? null}
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        onClose={() => {
          setArchiving(null);
        }}
        onArchived={archiving?.options.onArchived}
      />
      <ModuleDeleteDialog
        module={deleting?.target ?? null}
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        onOpenChange={(open) => {
          if (!open) setDeleting(null);
        }}
        onDeleted={deleting?.options.onDeleted}
      />
    </ModuleDialogContext.Provider>
  );
}
