// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared title-plus-description save pipeline (parity/specs/issues.md
// §13, ISS-145–147). Both fields report into one status the indicator
// renders; while a save is in flight the pipeline warns before the tab
// unloads. Peek mounts its own pipeline around the same fields.

import * as React from "react";

export type SaveStatus = "saving" | "saved";

export interface SavePipeline {
  status: SaveStatus;
  markSaving: () => void;
  markSaved: () => void;
}

export function useSavePipeline(): SavePipeline {
  const [status, setStatus] = React.useState<SaveStatus>("saved");
  React.useEffect(() => {
    if (status !== "saving") return;
    const guard = (event: BeforeUnloadEvent): void => {
      event.preventDefault();
    };
    window.addEventListener("beforeunload", guard);
    return () => {
      window.removeEventListener("beforeunload", guard);
    };
  }, [status]);
  return {
    status,
    markSaving: () => setStatus("saving"),
    markSaved: () => setStatus("saved"),
  };
}
