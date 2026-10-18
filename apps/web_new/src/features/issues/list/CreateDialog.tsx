// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Section "+" create modal (parity/specs/issues.md §2, ISS-012). One
// title field over the section's prefilled payload; the layout owns the
// create mutation and closes the modal after it settles. Loaded lazily so
// the dialog primitives stay out of the list's first-route chunk.

import { Button, Dialog, Input } from "@pidash/kit";
import * as React from "react";

export interface CreateDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSave: (name: string) => void;
}

/** Minimal create modal for the section "+" (title + prefilled payload). */
export function CreateDialog({
  open,
  onOpenChange,
  onSave,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSave: (name: string) => void;
}): React.ReactElement {
  const [title, setTitle] = React.useState("");
  React.useEffect(() => {
    if (open) setTitle("");
  }, [open]);
  return (
    <Dialog
      title="Create new work item"
      open={open}
      onOpenChange={(next) => onOpenChange(next)}
      actions={
        <>
          <Button variant="secondary" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button
            variant="primary"
            onClick={() => {
              const name = title.trim();
              if (name !== "") onSave(name);
            }}
            disabled={title.trim() === ""}
          >
            Save
          </Button>
        </>
      }
    >
      <Input
        label="Title"
        placeholder="Title"
        value={title}
        maxLength={255}
        onChange={(event) => setTitle(event.target.value)}
      />
    </Dialog>
  );
}
