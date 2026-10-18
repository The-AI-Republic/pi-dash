// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Lazy mount for the publish dialog (parity/specs/shell.md §7, SHELL-073).
// The sidebar row and the project quick actions are part of the initial
// chain, but the dialog only matters once someone picks "Publish project".
// Loading it on first open keeps the dialog primitives out of the shell's
// first paint (Quality gates: initial JS).
import * as React from "react";

import type { PublishProjectDialogProps } from "./PublishProjectDialog.js";

export type { PublishProjectDialogProps };

const LazyPublishProjectDialog = React.lazy(() =>
  import("./PublishProjectDialog.js").then((module) => ({ default: module.PublishProjectDialog }))
);

export type PublishProjectEntryProps = PublishProjectDialogProps;

export function PublishProjectEntry({
  projectName,
  open,
  onOpenChange,
}: PublishProjectEntryProps): React.ReactElement | null {
  if (!open) return null;
  return (
    <React.Suspense fallback={null}>
      <LazyPublishProjectDialog projectName={projectName} open={open} onOpenChange={onOpenChange} />
    </React.Suspense>
  );
}
