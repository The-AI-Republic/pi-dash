// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Lazy mount for the leave-project dialog (parity/specs/shell.md §7,
// SHELL-073; flow owned by SHELL-036). Both entry points sit in the
// initial chain and typically offer leaving to guests only, so the
// confirmation flow, its dialog primitives and the mutation wiring load
// on first open (Quality gates: initial JS).
import * as React from "react";

import type { LeaveProjectDialogProps } from "./LeaveProjectDialog.js";

export type { LeaveProjectDialogProps };

const LazyLeaveProjectDialog = React.lazy(() =>
  import("./LeaveProjectDialog.js").then((module) => ({ default: module.LeaveProjectDialog }))
);

export type LeaveProjectEntryProps = LeaveProjectDialogProps;

export function LeaveProjectEntry(props: LeaveProjectEntryProps): React.ReactElement | null {
  if (!props.open) return null;
  return (
    <React.Suspense fallback={null}>
      <LazyLeaveProjectDialog {...props} />
    </React.Suspense>
  );
}
