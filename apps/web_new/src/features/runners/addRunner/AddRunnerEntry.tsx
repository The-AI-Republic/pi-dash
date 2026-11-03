// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Overview entry for the add-runner dialog (RUN-006). One control opens the
// same modal on both runner scopes; the project-scoped area locks the project
// picker. The dev-machines page mounts the same modal through its own slice.

import { Button } from "@pidash/kit";
import * as React from "react";

import { useRunnersArea } from "../area.js";
import { AddRunnerModal } from "./AddRunnerModal.js";

export const ADD_RUNNER_LABEL = "Add runner";

export function AddRunnerEntry(): React.ReactElement {
  const area = useRunnersArea();
  const [open, setOpen] = React.useState(false);
  return (
    <>
      <Button variant="primary" onClick={() => setOpen(true)}>
        {ADD_RUNNER_LABEL}
      </Button>
      <AddRunnerModal open={open} onOpenChange={setOpen} projectId={area.scope.projectId} />
    </>
  );
}
