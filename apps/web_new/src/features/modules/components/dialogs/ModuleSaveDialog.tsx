// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module create/update dialog chrome (parity/specs/modules.md §7.1, §7.2,
// MOD-022–024). One controlled dialog for every entry point — the header
// Add action, the zero-state shortcut, and the row/card/detail menus that
// call this slice's openers. The form remounts on each open so a new
// visit starts from defaults (create) or the current record (update).
// Escape and Cancel close without writing; while a write flies, an
// outside press is swallowed so the form cannot be lost mid-submit.

import type { ModuleRow } from "@pidash/api-client";
import { Dialog } from "@pidash/kit";
import * as React from "react";

import { moduleDialogHeading, saveDialogDismisses, type ModuleDialogMode } from "../../lib/moduleForm.js";
import { ModuleForm, type ModuleFormProject } from "./ModuleForm.js";

export interface ModuleSaveDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  mode: ModuleDialogMode;
  workspaceSlug: string;
  currentProject: ModuleFormProject;
  /** Present exactly in update mode. */
  module?: ModuleRow | undefined;
}

export function ModuleSaveDialog(props: ModuleSaveDialogProps): React.ReactElement {
  const { open, onOpenChange, mode, workspaceSlug, currentProject, module } = props;
  const [session, setSession] = React.useState(0);
  const [submitting, setSubmitting] = React.useState(false);
  const wasOpen = React.useRef(open);
  React.useEffect(() => {
    if (open && !wasOpen.current) setSession((count) => count + 1);
    wasOpen.current = open;
  }, [open]);

  const form = (
    <ModuleForm
      key={session}
      mode={mode}
      workspaceSlug={workspaceSlug}
      currentProject={currentProject}
      module={module}
      onOpenChange={onOpenChange}
      onSubmittingChange={setSubmitting}
    />
  );

  return (
    <Dialog
      title={moduleDialogHeading(mode)}
      open={open}
      onOpenChange={(next, details) => {
        if (!saveDialogDismisses(submitting, details.reason)) return;
        onOpenChange(next);
      }}
      closeButton={false}
      initialFocus={false}
    >
      {form}
    </Dialog>
  );
}
